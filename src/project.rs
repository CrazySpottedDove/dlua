use crate::{log_info, log_warn, token::Token};
use logos::Logos;
use rayon::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;
use serde_json;

/// 每个文件的缓存信息：mtime（秒）和依赖列表
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileCache {
    pub mtime: u64,
    pub deps: Vec<PathBuf>,
}

/// 全量构建缓存（序列化到磁盘）
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BuildCache {
    pub files: HashMap<PathBuf, FileCache>,
}

#[derive(Debug, Clone)]
pub struct TokenWithText {
    pub kind: Token,
    pub text: String,
}

/// Project：根目录 + 缓存 + 所有源码 token 与依赖关系
#[derive(Debug)]
pub struct Project {
    pub root: PathBuf,
    pub files: HashMap<PathBuf, Vec<TokenWithText>>,
    pub require_relations: HashMap<PathBuf, Vec<PathBuf>>, // 正向：file -> deps
    pub reverse_require: HashMap<PathBuf, Vec<PathBuf>>,   // 反向：file -> dependents
    pub cache: BuildCache,
    pub cache_path: PathBuf,
}

impl Project {
    /// 加载目录，自动加载缓存（cache 成为 Project 成员），返回 Project。
    /// 退出时 Drop 会把 cache 持久化到 cache_path。
    pub fn load(
        root: impl AsRef<std::path::Path>,
        require_paths: Option<Vec<String>>,
        export_path: &PathBuf,
    ) -> std::io::Result<Self> {
        let root_path = root.as_ref().to_path_buf();
        let cache_path = export_path.join(".dlua_cache.json");

        // 初始化 Project（cache 先用 load_cache 填充）
        let mut project = Project {
            root: root_path.clone(),
            files: HashMap::new(),
            require_relations: HashMap::new(),
            reverse_require: HashMap::new(),
            cache: Self::load_cache(cache_path.to_str().unwrap_or(".dlua_cache.json")),
            cache_path,
        };

        let t0 = std::time::Instant::now();
        // 收集所有 lua 文件路径（整个目录）
        let all_lua_files: Vec<PathBuf> = WalkDir::new(&project.root)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "lua")
                    .unwrap_or(false)
            })
            .map(|e| e.path().to_path_buf())
            .collect();

        log_info!("Collected {} Lua files in {:.2?}", all_lua_files.len(), t0.elapsed());
        let t1 = std::time::Instant::now();

        // 查找变更文件（基于 cache 中记录的 mtime）
        let changed_files = Self::find_changed_files(&all_lua_files, &project.cache);
        log_info!("Found {} changed files", changed_files.len());

        // 从 cache 恢复依赖关系（尽可能），以便后面计算受影响集合
        for (path, file_cache) in &project.cache.files {
            if project.require_relations.contains_key(path) {
                continue;
            }
            if !file_cache.deps.is_empty() {
                project
                    .require_relations
                    .insert(path.clone(), file_cache.deps.clone());
                for dep in &file_cache.deps {
                    project
                        .reverse_require
                        .entry(dep.clone())
                        .or_insert_with(Vec::new)
                        .push(path.clone());
                }
            }
        }

        // 计算受影响集合：对每个变更文件，递归包含它的依赖（向下）与被它依赖的文件（向上）
        let affected_set = Self::compute_affected_set(&changed_files, &project.require_relations, &project.reverse_require);
        log_info!("Total affected files (changed + transitive deps/owners): {}", affected_set.len());

        // 并行读取与分词（对受影响的所有文件）
        let to_tokenize: Vec<PathBuf> = affected_set.iter().cloned().collect();
        let file_tokens: Vec<(PathBuf, Vec<TokenWithText>)> = to_tokenize
            .par_iter()
            .filter_map(|path| {
                let code = fs::read_to_string(path).ok()?;
                let mut lexer = Token::lexer(&code);
                let mut tokens_with_text = Vec::new();
                let src = lexer.source();
                while let Some(token_result) = lexer.next() {
                    match token_result {
                        Ok(token) => {
                            let span = lexer.span();
                            let text = src[span.start..span.end].to_string();
                            tokens_with_text.push(TokenWithText { kind: token, text });
                        }
                        Err(_) => {
                            // 忽略单个 token 错误；日志留给主线程
                        }
                    }
                }
                Some((path.clone(), tokens_with_text))
            })
            .collect();

        // 合并分词结果到 project.files（覆盖或新增）
        for (path, tokens) in file_tokens {
            project.files.insert(path, tokens);
        }

        log_info!("Tokenized {} Lua files in {:.2?}", project.files.len(), t1.elapsed());

        // 解析依赖关系：对刚分词的文件解析 require，并更新 require_relations / reverse_require / cache
        let require_paths = require_paths.unwrap_or_else(|| vec![".".to_string()]);
        let mut unresolved_requires: HashSet<PathBuf> = HashSet::new();

        for (file, tokens) in &project.files {
            let mut deps: Vec<PathBuf> = Vec::new();
            for req in Self::get_required_modules(tokens) {
                if let Some(dep_path) = project.resolve_require(&req, &require_paths, &mut unresolved_requires) {
                    deps.push(dep_path.clone());
                    project
                        .require_relations
                        .entry(file.clone())
                        .or_insert_with(Vec::new)
                        .push(dep_path.clone());
                    project
                        .reverse_require
                        .entry(dep_path)
                        .or_insert_with(Vec::new)
                        .push(file.clone());
                }
            }
            // 更新内存缓存
            Self::update_cache(&mut project.cache, file, deps);
        }

        // 对于仍然没有 require_relations 的文件（未分词且在 cache 中存在），恢复 cache 中的 deps
        for path in &all_lua_files {
            if project.require_relations.contains_key(path) {
                continue;
            }
            if let Some(file_cache) = project.cache.files.get(path) {
                for dep in &file_cache.deps {
                    project
                        .require_relations
                        .entry(path.clone())
                        .or_insert_with(Vec::new)
                        .push(dep.clone());
                    project
                        .reverse_require
                        .entry(dep.clone())
                        .or_insert_with(Vec::new)
                        .push(path.clone());
                }
            }
        }

        log_info!("Resolved require relations in {:.2?}", t1.elapsed());

        Ok(project)
    }

    /// 计算受影响集合（包含变更文件、本身依赖的文件、以及依赖它的文件；递归两方向）
    fn compute_affected_set(
        changed: &[PathBuf],
        require_relations: &HashMap<PathBuf, Vec<PathBuf>>,
        reverse_require: &HashMap<PathBuf, Vec<PathBuf>>,
    ) -> HashSet<PathBuf> {
        let mut visited: HashSet<PathBuf> = HashSet::new();
        let mut stack: Vec<PathBuf> = changed.iter().cloned().collect();

        while let Some(p) = stack.pop() {
            if !visited.insert(p.clone()) {
                continue;
            }
            // 向上：谁依赖我（需要重新编译）
            if let Some(owners) = reverse_require.get(&p) {
                for o in owners {
                    if !visited.contains(o) {
                        stack.push(o.clone());
                    }
                }
            }
            // 向下：我依赖的文件也可能需要重新编译 (例如宏变动会影响依赖)
            if let Some(deps) = require_relations.get(&p) {
                for d in deps {
                    if !visited.contains(d) {
                        stack.push(d.clone());
                    }
                }
            }
        }

        visited
    }

    /// 提取 tokens 中的静态 require 模块名
    fn get_required_modules(tokens_with_text: &Vec<TokenWithText>) -> Vec<String> {
        let mut found_modules = Vec::new();
        let mut require_found = false;
        let mut require_left_paren = false;

        for i in 0..tokens_with_text.len() {
            let token_with_text = &tokens_with_text[i];
            if token_with_text.kind == Token::Require {
                require_found = true;
            } else if require_found {
                if token_with_text.kind == Token::LParen {
                    require_left_paren = true;
                } else if require_left_paren {
                    if token_with_text.kind == Token::String {
                        let module_name = strip_quotes(&token_with_text.text).to_string();
                        found_modules.push(module_name);
                    } else if token_with_text.kind == Token::Whitespace {
                        // skip
                    } else if token_with_text.kind == Token::RParen {
                        require_found = false;
                        require_left_paren = false;
                    } else {
                        log_warn!("skip dynamic require {} in module parsing", token_with_text.text);
                        require_found = false;
                        require_left_paren = false;
                    }
                }
            }
        }
        found_modules
    }

    /// 解析 require 名称到实际文件路径（支持 search_paths 中带 '?' 或不带）
    fn resolve_require(&self, req: &str, search_paths: &[String], unresolved_requires: &mut HashSet<PathBuf>) -> Option<PathBuf> {
        let base_dir = &self.root;
        #[cfg(target_os = "windows")]
        let module_path = req.replace('.', "\\");
        #[cfg(not(target_os = "windows"))]
        let module_path = req.replace('.', "/");

        for search_path in search_paths {
            let candidate_base = if search_path.contains('?') {
                PathBuf::from(search_path.replace('?', &module_path))
            } else {
                PathBuf::from(search_path).join(&module_path)
            };

            let candidate_with_ext = base_dir.join(&candidate_base).with_extension("lua");
            if candidate_with_ext.exists() {
                return Some(candidate_with_ext);
            }

            let candidate_no_ext = base_dir.join(&candidate_base);
            if candidate_no_ext.exists() {
                return Some(candidate_no_ext);
            }
        }

        if !unresolved_requires.contains(&PathBuf::from(&module_path)) {
            unresolved_requires.insert(PathBuf::from(&module_path));
            log_warn!("Unable to resolve require '{}'", module_path);
        }
        None
    }

    /// 从磁盘加载缓存（JSON），失败则返回空缓存
    fn load_cache(path: &str) -> BuildCache {
        if let Ok(data) = fs::read_to_string(path) {
            if let Ok(cache) = serde_json::from_str(&data) {
                return cache;
            }
        }
        BuildCache {
            files: HashMap::new(),
        }
    }

    /// 获取文件的 mtime（秒），失败返回 0
    fn get_mtime(path: &PathBuf) -> u64 {
        fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|mtime| mtime.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|dur| dur.as_secs())
            .unwrap_or(0)
    }

    /// 找出需要重新分词/分析的文件（基于 mtime）
    fn find_changed_files(all_files: &Vec<PathBuf>, cache: &BuildCache) -> Vec<PathBuf> {
        let mut changed = Vec::new();
        for path in all_files {
            let mtime = Self::get_mtime(path);
            if let Some(file_cache) = cache.files.get(path) {
                if file_cache.mtime != mtime {
                    changed.push(path.clone());
                }
            } else {
                changed.push(path.clone());
            }
        }
        changed
    }

    /// 将缓存写盘（静默处理错误）
    fn save_cache(path: &str, cache: &BuildCache) {
        if let Ok(data) = serde_json::to_string_pretty(cache) {
            let _ = fs::write(path, data);
        }
    }

    /// 更新内存缓存（不立即写盘）
    fn update_cache(cache: &mut BuildCache, file: &PathBuf, deps: Vec<PathBuf>) {
        let mtime = Self::get_mtime(file);
        cache.files.insert(file.clone(), FileCache { mtime, deps });
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let path_str = self.cache_path.to_str().unwrap_or(".dlua_cache.json");
        Self::save_cache(path_str, &self.cache);
    }
}

/// 去除字符串两端的单/双引号
fn strip_quotes(s: &str) -> &str {
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        if (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
        {
            return &s[1..s.len() - 1];
        }
    }
    s
}