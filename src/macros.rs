// ...existing code...
use std::collections::HashMap;
use std::path::PathBuf;

use crate::{
    log_error,
    project::{Project, TokenWithText},
    token::Token,
};

#[derive(Debug)]
pub struct ProjectGlobalMacros {
    pub macro_map: HashMap<PathBuf, HashMap<String, Macro>>,
}

// 宏的定义：接受 0-n 个参数，返回字符串
#[derive(Clone, Debug)]
pub struct Macro {
    pub params: Vec<String>,
    pub template: String,
}

impl Macro {
    pub fn expand(&self, args: &[String]) -> String {
        let mut result = self.template.clone();
        for (i, param) in self.params.iter().enumerate() {
            if i < args.len() {
                result = result.replace(param, &args[i]);
            }
        }
        result
    }
}

impl ProjectGlobalMacros {
    pub fn new(project: &Project) -> Self {
        let mut project_global_macros = ProjectGlobalMacros {
            macro_map: HashMap::new(),
        };
        for (path, toks) in &project.files {
            project_global_macros.collect_global_macros(path, toks);
        }
        project_global_macros
    }

    /// 收集一个文件中所有的全局宏定义
    fn collect_global_macros(&mut self, path: &PathBuf, tokens: &Vec<TokenWithText>) {
        let mut macros = HashMap::new();
        let mut i = 0;
        while i < tokens.len() {
            let token = &tokens[i];
            if token.kind == Token::MacroComment {
                if let Some((name, macro_obj)) = Self::parse_global_macro(&mut i, tokens, &path) {
                    macros.insert(name, macro_obj);
                }
            } else {
                i += 1;
            }
        }
        self.macro_map.insert(path.clone(), macros);
    }

    // 核心的解析宏功能
    fn parse_macro_core(index: &mut usize, tokens: &Vec<TokenWithText>, path: &PathBuf) -> Option<(String, Macro)> {
        let max_index = tokens.len() - 1;
        match tokens[*index].kind {
            Token::Ident => {
                // 变量宏 NAME = VALUE
                let name = tokens[*index].text.clone();
                *index += 1;

                // 跳过可能的空白
                while *index <= max_index && tokens[*index].kind == Token::Whitespace {
                    *index += 1;
                }

                if *index > max_index {
                    log_error!("{path:?}: unexpected end of tokens after macro name {name}");
                }

                // 接下来应该是等号
                if tokens[*index].kind != Token::Assign {
                    log_error!("{path:?}: expected '=' after macro name {name}, found {}", tokens[*index].text);
                }
                *index += 1;

                // 跳过可能的空白
                while *index <= max_index && tokens[*index].kind == Token::Whitespace {
                    *index += 1;
                }

                // 接下来是宏的值，直到行尾
                let mut value = String::new();
                while *index <= max_index {
                    let tk = &tokens[*index];
                    if tk.kind == Token::Whitespace {
                        break;
                    }
                    value.push_str(&tk.text);
                    *index += 1;
                }

                // 创建宏对象
                let macro_obj = Macro {
                    params: Vec::new(),
                    template: value,
                };
                Some((name, macro_obj))
            }
            Token::FunctionKw => {
                // 函数宏 function NAME(args) ... end
                *index += 1;

                // 跳过可能的空白
                while *index <= max_index && tokens[*index].kind == Token::Whitespace {
                    *index += 1;
                }

                if *index > max_index {
                    log_error!("{path:?}: unexpected end of tokens after 'function' keyword");
                }

                // 接下来应该是宏的名称
                if tokens[*index].kind != Token::Ident {
                    log_error!("{path:?}: expected macro name after 'function' keyword, found {}", tokens[*index].text);
                }

                let name = tokens[*index].text.clone();
                *index += 1;

                // 跳过可能的空白
                while *index <= max_index && tokens[*index].kind == Token::Whitespace {
                    *index += 1;
                }

                if *index > max_index {
                    log_error!("{path:?}: unexpected end of tokens after macro name {name}");
                }

                // 接下来应该是左括号
                if tokens[*index].kind != Token::LParen {
                    log_error!("{path:?}: expected '(' after macro name {name}, found {}", tokens[*index].text);
                }
                *index += 1;

                // 收集参数列表
                let mut params = Vec::new();
                while *index <= max_index && tokens[*index].kind != Token::RParen {
                    if tokens[*index].kind == Token::Ident {
                        params.push(tokens[*index].text.clone());
                    }
                    *index += 1;
                }

                // 跳过右括号
                if *index > max_index {
                    log_error!("{path:?}: unexpected end of tokens while parsing parameters for macro {name}");
                }

                if tokens[*index].kind != Token::RParen {
                    log_error!("{path:?}: expected ')' after parameters for macro {name}, found {}", tokens[*index].text);
                }
                *index += 1;

                // 收集函数体，直到遇到 'end'
                let mut body = String::new();
                while *index <= max_index {
                    let tk = &tokens[*index];
                    if tk.kind == Token::EndKw {
                        *index += 1;
                        break;
                    }
                    if tk.kind == Token::ReturnKw {
                        // 跳过 return 关键字
                        *index += 1;
                        continue;
                    }
                    body.push_str(&tk.text);
                    *index += 1;
                }

                // 创建宏对象
                let macro_obj = Macro {
                    params,
                    template: body,
                };
                Some((name, macro_obj))
            }
            _ => {
                log_error!("{path:?}: expected macro name or 'function' keyword, found {}", tokens[*index].text);
            }
        }
    }

    // 用于解析全局宏，在识别到 Token::MacroComment 后调用
    fn parse_global_macro(
        index: &mut usize,
        tokens: &Vec<TokenWithText>,
        path: &PathBuf,
    ) -> Option<(String, Macro)> {
        // 跳过 Token::MacroComment
        *index += 1;

        let max_index = tokens.len() - 1;

        if *index > max_index {
            log_error!("{path:?}: unexpected end of tokens after macro comment");
        }

        // 下一个 token 应当是 Token::Whitespace
        if tokens[*index].kind != Token::Whitespace {
            log_error!("{path:?}: expected whitespace after macro comment, found {}", tokens[*index].text);
        }

        *index += 1;

        // 接下来，如果下一个 token 是 Token::LocalKw，说明是一个局部宏，我们跳过。
        if *index > max_index {
            log_error!("{path:?}: unexpected end of tokens after macro comment");
        }
        if tokens[*index].kind == Token::LocalKw {
            // 跳过局部宏定义
            return None;
        }

        // 现在，已经可以确定这是一个全局宏定义。调用宏解析逻辑即可。
        Self::parse_macro_core(index, tokens, path)
    }

    // 用于解析局部宏，并丢弃全局宏或局部宏的语句，在识别到 Token::MacroComment 后调用
    fn parse_local_macro(
        index: &mut usize,
        tokens: &Vec<TokenWithText>,
        path: &PathBuf,
    ) -> Option<(String, Macro)> {
        // 跳过 Token::MacroComment
        *index += 1;

        let max_index = tokens.len() - 1;

        // 下一个 token 应当是 Token::Whitespace
        if *index > max_index {
            log_error!("{path:?}: unexpected end of tokens after macro comment");
        }

        if tokens[*index].kind != Token::Whitespace {
            log_error!("{path:?}: expected whitespace after macro comment, found {}", tokens[*index].text);
        }

        *index += 1;

        if *index > max_index {
            log_error!("{path:?}: unexpected end of tokens after macro comment");
        }

        if tokens[*index].kind == Token::LocalKw {
            *index += 1;

            if *index > max_index {
                log_error!("{path:?}: unexpected end of tokens after 'local' keyword");
            }

            // 跳过空白
            while *index <= max_index && tokens[*index].kind == Token::Whitespace {
                *index += 1;
            }

            // 如果是局部宏，返回解析结果
            Self::parse_macro_core(index, tokens, path)
        } else {
            // 否则跳过全局宏定义
            Self::parse_macro_core(index, tokens, path);
            None
        }
    }

    // 展开所有文件，用户传入：user_level（编译等级），以及宏名称到等级的映射level_map
    // 不再维护 HashMap，直接写入文件
    pub fn expand_all_with_levels(
        &self,
        project: &Project,
        user_level: u8,
        level_map: &HashMap<String, u8>,
        export_path: &PathBuf,
    ) {
        let project_root_path = &project.root;

        for path in project.files.keys() {
            let code = self.expand_file_with_levels(project, path, user_level, level_map);
            // 应该使用 export_path 作为根目录，保持相对路径不变
            let relative_path = path.strip_prefix(project_root_path).unwrap();
            let out_path = export_path.join(relative_path).with_extension("lua");
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(out_path, code).unwrap();
        }
    }

    fn expand_file_with_levels(
        &self,
        project: &Project,
        file_path: &PathBuf,
        user_level: u8,
        level_map: &HashMap<String, u8>,
    ) -> String {
        let tokens = match project.files.get(file_path) {
            Some(v) => v,
            None => return String::new(),
        };

        // 合并 require 的宏（依赖先，当前后；后者覆盖）
        let mut merged: HashMap<String, Macro> = HashMap::new();
        if let Some(reqs) = project.require_relations.get(file_path) {
            for dep in reqs {
                if let Some(m) = self.macro_map.get(dep) {
                    for (k, v) in m {
                        merged.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        if let Some(m) = self.macro_map.get(file_path) {
            for (k, v) in m {
                merged.insert(k.clone(), v.clone());
            }
        }

        let mut i = 0;
        let mut out = String::new();

        let mut scope_stack: Vec<HashMap<String, Macro>> = Vec::new();

        while i < tokens.len() {
            let tk = &tokens[i];
            match tk.kind {
                Token::FunctionKw | Token::DoKw | Token::ThenKw | Token::RepeatKw => {
                    // 进入新作用域
                    scope_stack.push(HashMap::new());
                    out.push_str(&tk.text);
                    i += 1;
                }
                Token::EndKw | Token::UntilKw => {
                    // 退出作用域
                    scope_stack.pop();
                    out.push_str(&tk.text);
                    i += 1;
                }
                Token::ElseKw | Token::ElseIfKw => {
                    // 退出并进入新的作用域
                    scope_stack.pop();
                    scope_stack.push(HashMap::new());
                    out.push_str(&tk.text);
                    i += 1;
                }
                // 还是要检查宏，如果是局部宏，则允许它在当前作用域生效。
                Token::MacroComment => {
                    if let Some((name, macro_obj)) = Self::parse_local_macro(&mut i, tokens, file_path) {
                        if let Some(top_scope) = scope_stack.last_mut() {
                            top_scope.insert(name, macro_obj);
                        } else {
                            merged.insert(name, macro_obj);
                        }
                    }
                }
                Token::Ident => {
                    // 检测宏调用
                    if let Some(macro_obj) = scope_stack
                        .iter()
                        .rev()
                        .find_map(|scope| scope.get(&tk.text))
                        .or_else(|| merged.get(&tk.text))
                    {
                        // 发现宏调用
                        let name = tk.text.clone();
                        i += 1;

                        // 先检查是函数宏还是变量宏
                        if macro_obj.params.is_empty() {
                            out.push_str(&macro_obj.expand(&[]));
                        } else {
                            // 函数宏，收集参数
                            let mut args: Vec<String> = Vec::new();
                            // 跳过可能的空白
                            while i < tokens.len() && tokens[i].kind == Token::Whitespace {
                                i += 1;
                            }

                            if i >= tokens.len() {
                                log_error!("{file_path:?}: expected '(' after macro name {name}, found end of tokens");
                            }

                            // 接下来应该是左括号
                            if  tokens[i].kind != Token::LParen {
                                log_error!("{file_path:?}: expected '(' after macro name {name}, found {}", tokens[i].text);
                            }
                            i += 1;

                            // 收集参数，直到右括号
                            let mut current_arg = String::new();
                            let mut paren_level = 0;
                            while i < tokens.len() {
                                let tk = &tokens[i];
                                if tk.kind == Token::RParen && paren_level == 0 {
                                    if !current_arg.trim().is_empty() {
                                        args.push(current_arg.trim().to_string());
                                    }
                                    i += 1;
                                    break;
                                } else if tk.kind == Token::Comma && paren_level == 0 {
                                    args.push(current_arg.trim().to_string());
                                    current_arg.clear();
                                    i += 1;
                                } else {
                                    if tk.kind == Token::LParen {
                                        paren_level += 1;
                                    } else if tk.kind == Token::RParen {
                                        paren_level -= 1;
                                    }
                                    current_arg.push_str(&tk.text);
                                    i += 1;
                                }
                            }
                            // 展开宏
                            let expanded = macro_obj.expand(&args);
                            out.push_str(&expanded);
                        }
                    } else {
                        // 普通标识符，直接输出
                        out.push_str(&tk.text);
                        i += 1;
                    }
                }
                _ => {
                    out.push_str(&tk.text);
                    i += 1;
                }
            }
        }
        out
    }
}

// 解析 "-- @if name ..." 的 name
fn parse_if_name(comment_text: &str) -> &str {
    // 去掉开头 "--" 后的部分
    let s = comment_text.trim_start();
    let s = s.strip_prefix("--").unwrap_or(s).trim_start();
    // 去掉 "@if"
    let s = s.strip_prefix("@if").unwrap_or(s).trim_start();
    // 取到第一个空白或行尾
    let bytes = s.as_bytes();
    let mut end = bytes.len();
    for (idx, b) in bytes.iter().enumerate() {
        if b.is_ascii_whitespace() {
            end = idx;
            break;
        }
    }
    &s[..end]
}
// ...existing code...
