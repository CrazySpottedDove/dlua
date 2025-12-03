use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::project::File;
use crate::{
    log_error,
    project::Project,
    token::Token,
};

#[derive(Debug)]
pub struct Processor {
    pub global_macro_map: HashMap<PathBuf, HashMap<String, Macro>>,
    project: Project,
}

// 宏的定义：接受 0-n 个参数，返回字符串
#[derive(Clone, Debug)]
pub struct Macro {
    pub params: Vec<String>,
    pub template: String,
}

impl Macro {
    pub fn expand(&self, args: &[String]) -> String {
        // 按标识符边界替换，避免在其它单词内误替换（例如避免把 "w" 替换到 "scroller_width"）
        if self.params.is_empty() || args.is_empty() {
            return self.template.clone();
        }

        let mut out = String::with_capacity(self.template.len());
        let mut chars = self.template.chars().peekable();

        while let Some(ch) = chars.next() {
            // 识别标识符开始（字母或下划线）
            if ch.is_ascii_alphabetic() || ch == '_' {
                let mut ident = String::new();
                ident.push(ch);
                while let Some(&nc) = chars.peek() {
                    if nc.is_ascii_alphanumeric() || nc == '_' {
                        ident.push(nc);
                        chars.next();
                    } else {
                        break;
                    }
                }
                // 在 params 中查找完全匹配的参数名，并替换为对应的实参（按索引）
                let mut replaced = false;
                for (i, param) in self.params.iter().enumerate() {
                    if &ident == param {
                        if i < args.len() {
                            out.push_str(&args[i]);
                        } else {
                            // 如果缺少实参，保留原标识符
                            out.push_str(&ident);
                        }
                        replaced = true;
                        break;
                    }
                }
                if !replaced {
                    out.push_str(&ident);
                }
            } else {
                // 非标识符字符直接输出
                out.push(ch);
            }
        }

        out
    }
}

impl Processor {
    pub fn new(project: Project) -> Self {
        let mut processor = Processor {
            global_macro_map: HashMap::new(),
            project,
        };
        processor.collect_global_macros();
        processor
    }

    fn collect_global_macros(&mut self) {
        let global_macros: HashMap<PathBuf, HashMap<String, Macro>> = self
            .project
            .files
            .par_iter_mut() // 使用并行迭代器
            .map(|(path, file)| {
                let macros = file.parse_global_macros();
                (path.clone(), macros)
            })
            .collect(); // 收集结果到 HashMap

        self.global_macro_map = global_macros;
    }

    // 展开所有文件，用户传入：user_level（编译等级），以及宏名称到等级的映射level_map
    // 不再维护 HashMap，直接写入文件
    pub fn expand_all_with_levels(
        &mut self,
        user_level: u8,
        level_map: &HashMap<String, u8>,
        export_path: &PathBuf,
    ) {
        let project_root_path = self.project.root.clone(); // 克隆不可变引用
        let global_macro_map = &self.global_macro_map; // 引用全局宏映射

        self.project
            .files
            .par_iter_mut() // 使用并行迭代器
            .for_each(|(path, file)| {
                file.set_stacks(&self.project.require_relations, global_macro_map);
                file.expand(user_level, level_map);

                // 应该使用 export_path 作为根目录，保持相对路径不变
                let relative_path = path.strip_prefix(&project_root_path).unwrap();
                let out_path = export_path.join(relative_path).with_extension("lua");
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(out_path, &file.output).unwrap();
            });
    }
}

impl File {
    /// 设置作用域栈和遮蔽栈，加载全局宏
    pub fn set_stacks(
        &mut self,
        require_relations: &HashMap<PathBuf, Vec<PathBuf>>,
        global_macro_map: &HashMap<PathBuf, HashMap<String, Macro>>,
    ) {
        self.scope_stack.push(HashMap::new());
        self.shadow_stack.push(HashSet::new());
        let global_scope_stack = &mut self.scope_stack[0];
        if let Some(reqs) = require_relations.get(&self.path) {
            for dep in reqs {
                if let Some(m) = global_macro_map.get(dep) {
                    for (k, v) in m {
                        global_scope_stack.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        if let Some(m) = global_macro_map.get(&self.path) {
            for (k, v) in m {
                global_scope_stack.insert(k.clone(), v.clone());
            }
        }
    }

    // 重置解析索引和输出缓冲区
    fn reset_parse_index(&mut self) {
        self.parse_index = 0;
        self.output.clear();
    }

    fn consume(&mut self) {
        self.output.push_str(&self.tokens[self.parse_index].text);
        self.parse_index += 1;
    }

    fn check_eof(&self) {
        if self.parse_index >= self.tokens.len() {
            log_error!("{}: unexpected end of tokens", self.current_pos());
        }
    }

    fn consume_whitespace(&mut self) {
        while self.parse_index < self.tokens.len()
            && self.tokens[self.parse_index].kind == Token::Whitespace
        {
            self.consume();
        }
    }

    fn current_kind(&self) -> &Token {
        &self.tokens[self.parse_index].kind
    }

    fn finished(&self) -> bool {
        self.parse_index >= self.tokens.len()
    }

    fn enter_scope(&mut self) {
        self.scope_stack.push(HashMap::new());
        self.shadow_stack.push(HashSet::new());
    }

    fn exit_scope(&mut self) {
        self.scope_stack.pop();
        self.shadow_stack.pop();
    }

    /// 解析函数参数列表，加入 shadow_stack
    fn parse_function_args(&mut self, is_global: bool) {
        self.consume(); // FunctionKw
        self.consume_whitespace();
        self.check_eof();

        // 可能存在函数名
        if self.current_kind() == &Token::Ident {
            if is_global {
                // 全局函数，加入全局作用域
                self.shadow_stack[0].insert(self.tokens[self.parse_index].text.clone());
            } else {
                // 局部函数，加入当前作用域
                if let Some(current_shadow) = self.shadow_stack.last_mut() {
                    current_shadow.insert(self.tokens[self.parse_index].text.clone());
                } else {
                    log_error!(
                        "{}: internal error: shadow_stack is empty when inserting function name",
                        self.current_pos()
                    );
                }
            }
            self.consume(); // 函数名
        }

        // 函数名确定后，进入下一层作用域
        self.scope_stack.push(HashMap::new());
        self.shadow_stack.push(HashSet::new());

        self.consume_whitespace();
        self.check_eof();

        // 左括号
        if self.current_kind() != &Token::LParen {
            log_error!(
                "{}: expected '(' after function name, found {}",
                self.current_pos(),
                self.tokens[self.parse_index].text
            );
        }
        self.consume(); // LParen
        self.consume_whitespace();
        self.check_eof();

        while !self.finished() && self.current_kind() != &Token::RParen {
            if self.current_kind() == &Token::Ident {
                let param_name = self.tokens[self.parse_index].text.clone();
                if let Some(current_shadow) = self.shadow_stack.last_mut() {
                    current_shadow.insert(param_name);
                } else {
                    log_error!(
                        "{}: internal error: shadow_stack is empty when inserting function parameter",
                        self.current_pos()
                    );
                }
                self.consume(); // 参数名
            }

            self.consume_whitespace();
            self.check_eof();

            if self.current_kind() == &Token::Comma {
                self.consume(); // Comma
            }
            self.consume_whitespace();
            self.check_eof();
        }

        // 右括号
        if self.current_kind() != &Token::RParen {
            log_error!(
                "{}: expected ')' after function parameters, found {}",
                self.current_pos(),
                self.tokens[self.parse_index].text
            );
        }

        self.consume(); // RParen
    }

    /// 解析 for 变量列表，加入 shadow_stack
    fn parse_for_variables(&mut self) {}

    fn skip(&mut self) {
        self.parse_index += 1;
    }

    fn skip_whitespace(&mut self) {
        while self.parse_index < self.tokens.len()
            && self.tokens[self.parse_index].kind == Token::Whitespace
        {
            self.parse_index += 1;
        }
    }

    // 解析宏
    fn parse_macro_core(&mut self, is_global: bool) {
        // 首先，应该检查这是一个变量宏还是有一个函数宏
        match self.current_kind() {
            &Token::Ident => {
                // 变量宏
                let macro_name = self.tokens[self.parse_index].text.clone();
                self.skip(); // 跳过宏名称
                self.skip_whitespace();
                self.check_eof();

                // 接下来应该是个等号
                if self.current_kind() != &Token::Assign {
                    log_error!(
                        "{}: expected '=' after macro name {}, found {}",
                        self.current_pos(),
                        macro_name,
                        self.tokens[self.parse_index].text
                    );
                }
                self.skip(); // 跳过等号
                self.skip_whitespace();
                self.check_eof();

                // 接下来是宏的值
                let mut template = String::new();
                while !self.finished() {
                    if self.current_kind() == &Token::Whitespace {
                        break;
                    }
                    template.push_str(&self.tokens[self.parse_index].text);
                    self.skip();
                }

                // 不允许在局部作用域定义全局宏
                if is_global {
                    if self.scope_stack.len() > 1 {
                        log_error!(
                            "{}: trying to define global macro {} inside local scope",
                            self.current_pos(),
                            macro_name
                        );
                    }
                }

                if let Some(macro_map) = self.scope_stack.last_mut() {
                    let macro_obj = Macro {
                        params: Vec::new(),
                        template,
                    };
                    macro_map.insert(macro_name, macro_obj);
                } else {
                    log_error!(
                        "{}: internal error: scope_stack is empty when inserting local variable macro",
                        self.current_pos()
                    );
                }
            }
            &Token::FunctionKw => {
                // 函数宏
                self.skip(); // 跳过 FunctionKw
                self.skip_whitespace();
                self.check_eof();

                // 接下来应该是宏名称
                if self.current_kind() != &Token::Ident {
                    log_error!(
                        "{}: expected macro name after 'function' keyword, found {}",
                        self.current_pos(),
                        self.tokens[self.parse_index].text
                    );
                }

                let macro_name = self.tokens[self.parse_index].text.clone();
                self.skip(); // 跳过宏名称
                self.skip_whitespace();
                self.check_eof();

                // 左括号
                if self.current_kind() != &Token::LParen {
                    log_error!(
                        "{}: expected '(' after macro name {}, found {}",
                        self.current_pos(),
                        macro_name,
                        self.tokens[self.parse_index].text
                    );
                }
                self.skip(); // 跳过 LParen
                self.skip_whitespace();
                self.check_eof();

                // 解析参数列表
                let mut params: Vec<String> = Vec::new();
                while !self.finished() && self.current_kind() != &Token::RParen {
                    if self.current_kind() == &Token::Ident {
                        let param_name = self.tokens[self.parse_index].text.clone();
                        params.push(param_name);
                        self.skip(); // 跳过参数名
                    }

                    self.skip_whitespace();
                    self.check_eof();

                    if self.current_kind() == &Token::Comma {
                        self.skip(); // 跳过 Comma
                    }
                    self.skip_whitespace();
                    self.check_eof();
                }
                // 右括号
                if self.current_kind() != &Token::RParen {
                    log_error!(
                        "{}: expected ')' after macro parameters, found {}",
                        self.current_pos(),
                        self.tokens[self.parse_index].text
                    );
                }
                self.skip(); // 跳过 RParen
                self.skip_whitespace();
                self.check_eof();
                // 接下来是宏的模板
                let mut template = String::new();
                let mut end_found = false;
                while !self.finished() {
                    if self.current_kind() == &Token::EndKw {
                        self.skip();
                        self.skip_whitespace();
                        end_found = true;
                        break;
                    }
                    if self.current_kind() == &Token::ReturnKw {
                        self.skip();
                        continue;
                    }
                    template.push_str(&self.tokens[self.parse_index].text);
                    self.skip();
                }

                if !end_found {
                    log_error!(
                        "{}: expected 'end' keyword to terminate macro function definition",
                        self.current_pos()
                    );
                }
                if is_global {
                    if self.scope_stack.len() > 1 {
                        log_error!(
                            "{}: trying to define global macro {} inside local scope",
                            self.current_pos(),
                            macro_name
                        );
                    }
                }

                if let Some(macro_map) = self.scope_stack.last_mut() {
                    let macro_obj = Macro { params, template };
                    macro_map.insert(macro_name, macro_obj);
                } else {
                    log_error!(
                        "{}: internal error: scope_stack is empty when inserting local function macro",
                        self.current_pos()
                    );
                }
            }
            _ => {
                log_error!(
                    "{}: expected macro name or 'function' keyword after 'local' keyword, found {}",
                    self.current_pos(),
                    self.tokens[self.parse_index].text
                );
            }
        }
    }

    // 往前读取，并跳过这个宏定义，不加入宏记录，也不对 output 产生写入
    fn ignore_macro_core(&mut self) {
        // 首先，应该检查这是一个变量宏还是有一个函数宏
        match self.current_kind() {
            &Token::Ident => {
                // 变量宏
                let macro_name = self.tokens[self.parse_index].text.clone();
                self.skip(); // 跳过宏名称
                self.skip_whitespace();
                self.check_eof();

                // 接下来应该是个等号
                if self.current_kind() != &Token::Assign {
                    log_error!(
                        "{}: expected '=' after macro name {}, found {}",
                        self.current_pos(),
                        macro_name,
                        self.tokens[self.parse_index].text
                    );
                }
                self.skip(); // 跳过等号
                self.skip_whitespace();
                self.check_eof();

                // 接下来是宏的值
                // let mut template = String::new();
                while !self.finished() {
                    if self.current_kind() == &Token::Whitespace {
                        break;
                    }
                    // template.push_str(&self.tokens[self.parse_index].text);
                    self.skip();
                }
                // if let Some(macro_map) = self.scope_stack.last_mut() {
                //     let macro_obj = Macro {
                //         params: Vec::new(),
                //         template,
                //     };
                //     macro_map.insert(macro_name, macro_obj);
                // } else {
                //     log_error!(
                //         "{}: internal error: scope_stack is empty when inserting local variable macro",
                //         self.current_pos()
                //     );
                // }
            }
            &Token::FunctionKw => {
                // 函数宏
                self.skip(); // 跳过 FunctionKw
                self.skip_whitespace();
                self.check_eof();

                // 接下来应该是宏名称
                if self.current_kind() != &Token::Ident {
                    log_error!(
                        "{}: expected macro name after 'function' keyword, found {}",
                        self.current_pos(),
                        self.tokens[self.parse_index].text
                    );
                }

                let macro_name = self.tokens[self.parse_index].text.clone();
                self.skip(); // 跳过宏名称
                self.skip_whitespace();
                self.check_eof();

                // 左括号
                if self.current_kind() != &Token::LParen {
                    log_error!(
                        "{}: expected '(' after macro name {}, found {}",
                        self.current_pos(),
                        macro_name,
                        self.tokens[self.parse_index].text
                    );
                }
                self.skip(); // 跳过 LParen
                self.skip_whitespace();
                self.check_eof();

                // 解析参数列表
                // let mut params: Vec<String> = Vec::new();
                while !self.finished() && self.current_kind() != &Token::RParen {
                    if self.current_kind() == &Token::Ident {
                        // let param_name = self.tokens[self.parse_index].text.clone();
                        // params.push(param_name);
                        self.skip(); // 跳过参数名
                    }

                    self.skip_whitespace();
                    self.check_eof();

                    if self.current_kind() == &Token::Comma {
                        self.skip(); // 跳过 Comma
                    }
                    self.skip_whitespace();
                    self.check_eof();
                }
                // 右括号
                if self.current_kind() != &Token::RParen {
                    log_error!(
                        "{}: expected ')' after macro parameters, found {}",
                        self.current_pos(),
                        self.tokens[self.parse_index].text
                    );
                }
                self.skip(); // 跳过 RParen
                self.skip_whitespace();
                self.check_eof();
                // 接下来是宏的模板
                // let mut template = String::new();
                let mut end_found = false;
                while !self.finished() {
                    if self.current_kind() == &Token::EndKw {
                        self.skip();
                        self.skip_whitespace();
                        end_found = true;
                        break;
                    }
                    if self.current_kind() == &Token::ReturnKw {
                        self.skip();
                        continue;
                    }
                    // template.push_str(&self.tokens[self.parse_index].text);
                    self.skip();
                }

                if !end_found {
                    log_error!(
                        "{}: expected 'end' keyword to terminate macro function definition",
                        self.current_pos()
                    );
                }
                // if let Some(macro_map) = self.scope_stack.last_mut() {
                //     let macro_obj = Macro { params, template };
                //     macro_map.insert(macro_name, macro_obj);
                // } else {
                //     log_error!(
                //         "{}: internal error: scope_stack is empty when inserting local function macro",
                //         self.current_pos()
                //     );
                // }
            }
            _ => {
                log_error!(
                    "{}: expected macro name or 'function' keyword after 'local' keyword, found {}",
                    self.current_pos(),
                    self.tokens[self.parse_index].text
                );
            }
        }
    }

    /// 解析宏定义
    /// is_global: 尝试解析的是否是全局宏
    fn parse_local_macro(&mut self) {
        // 跳过 Token::MacroComment
        self.skip();

        // 跳过宏注释后的空白
        self.skip_whitespace();
        self.check_eof();

        // 现在可以检查宏是全局的还是局部的。如果是局部的，当前必然有 current_kind() == Token::LocalKw
        if self.current_kind() != &Token::LocalKw {
            self.ignore_macro_core();
            return;
        }
        self.skip(); // 跳过 Token::LocalKw
        self.skip_whitespace();
        self.check_eof();

        // 接下来就是宏的解析核心了，调用 parse_macro_core
        self.parse_macro_core(false);
    }

    fn parse_global_macro(&mut self) {
        self.skip(); // 跳过 Token::MacroComment
        self.skip_whitespace();
        self.check_eof();

        // 现在可以检查宏是全局的还是局部的。
        if self.current_kind() == &Token::LocalKw {
            self.skip();
            self.skip_whitespace();
            self.check_eof();
            self.ignore_macro_core();
            return;
        }
        // 接下来就是宏的解析核心了，调用 parse_macro_core
        self.parse_macro_core(true);
    }

    pub fn parse_global_macros(&mut self) -> HashMap<String, Macro> {
        self.reset_parse_index();
        self.scope_stack.push(HashMap::new());
        while !self.finished() {
            match self.current_kind() {
                &Token::MacroComment => {
                    self.parse_global_macro();
                }
                _ => {
                    self.skip();
                }
            }
        }
        self.scope_stack.pop().unwrap()
    }

    fn parse_alias(&mut self) {
        self.skip(); // 跳过 Token::AliasComment
        self.skip_whitespace();
        self.check_eof();

        // 接下来必须是 Token::LocalKw，因为我们禁用了全局的 alias
        if self.current_kind() != &Token::LocalKw {
            log_error!(
                "{}: expected 'local' keyword after alias comment, found {}",
                self.current_pos(),
                self.tokens[self.parse_index].text
            );
        }

        self.skip(); // 跳过 Token::LocalKw
        self.skip_whitespace();
        self.check_eof();

        // 现在应该是 Token::Ident，表示别名的名称
        if self.current_kind() != &Token::Ident {
            log_error!(
                "{}: expected identifier after 'local' keyword in alias comment, found {}",
                self.current_pos(),
                self.tokens[self.parse_index].text
            );
        }

        let alias_name = self.tokens[self.parse_index].text.clone();
        self.skip(); // 跳过别名名称
        self.skip_whitespace();
        self.check_eof();

        // 接下来应该是 Token::Assign
        if self.current_kind() != &Token::Assign {
            log_error!(
                "{}: expected '=' after alias name {}, found {}",
                self.current_pos(),
                alias_name,
                self.tokens[self.parse_index].text
            );
        }

        self.skip(); // 跳过 '='
        self.skip_whitespace();
        self.check_eof();

        // 现在应该是 Token::Ident，表示被别名的宏名称
        if self.current_kind() != &Token::Ident {
            log_error!(
                "{}: expected identifier after '=' in alias comment for alias {}, found {}",
                self.current_pos(),
                alias_name,
                self.tokens[self.parse_index].text
            );
        }

        let target_name = self.tokens[self.parse_index].text.clone();
        self.skip(); // 跳过被别名的宏名称
        self.skip_whitespace();
        // 查找被别名的宏定义
        let stack_size = self.scope_stack.len();
        let mut macro_obj_opt: Option<Macro> = None;
        // 先在 scope_stack 中反向查找
        for j in (0..stack_size).rev() {
            if let Some(macro_obj) = self.scope_stack[j].get(&target_name) {
                // 找到后复制一份
                // 再在 shadow_stack 中检查是否被遮蔽
                for k in (j..stack_size).rev() {
                    if self.shadow_stack[k].contains(&target_name) {
                        log_error!(
                            "{}: macro {} is shadowed in current scope, cannot create alias {}",
                            self.current_pos(),
                            target_name,
                            alias_name
                        );
                    }
                }
                macro_obj_opt = Some(macro_obj.clone());
                break;
            }
        }

        if let Some(macro_obj) = macro_obj_opt {
            // 注册别名到当前作用域
            if let Some(current_scope) = self.scope_stack.last_mut() {
                current_scope.insert(alias_name, macro_obj);
            } else {
                log_error!(
                    "{}: internal error: scope_stack is empty when inserting alias macro",
                    self.current_pos()
                );
            }
        } else {
            log_error!(
                "{}: macro {} not found for alias {}",
                self.current_pos(),
                target_name,
                alias_name
            );
        }
    }

    /// 解析局部变量声明，加入 shadow_stack
    fn parse_local(&mut self) {
        self.consume(); // 跳过 Token::LocalKw
        self.consume_whitespace();
        self.check_eof();

        if self.current_kind() == &Token::FunctionKw {
            self.parse_function_args(false);
            return;
        }

        if self.current_kind() != &Token::Ident {
            log_error!(
                "{}: expected identifier after 'local' keyword, found {}",
                self.current_pos(),
                self.tokens[self.parse_index].text
            );
        }

        // 收集局部变量名
        let var_name = self.tokens[self.parse_index].text.clone();
        if let Some(current_shadow) = self.shadow_stack.last_mut() {
            current_shadow.insert(var_name);
        } else {
            log_error!(
                "{}: internal error: shadow_stack is empty when inserting local variable",
                self.current_pos()
            );
        }
        self.consume(); // 变量名
    }

    /// 解析标识符，尝试作为宏调用或普通标识符处理
    fn parse_ident(&mut self) {
        let name = self.tokens[self.parse_index].text.clone();
        let current_parse_index = self.parse_index;

        // 先检查是在给这个 ident 赋值，还是说使用它
        self.skip();
        self.skip_whitespace();
        self.check_eof();

        // 如果是赋值，此时应该是 Token::Assign
        if self.current_kind() == &Token::Assign {
            // 加到全局变量中
            self.shadow_stack[0].insert(name);
            self.parse_index = current_parse_index; // 回到 ident 位置
            self.consume(); // ident
            self.consume_whitespace();
            self.consume();
            return;
        }

        // 否则，尝试进行宏展开。
        let stack_size = self.scope_stack.len();
        let mut macro_obj_opt: Option<Macro> = None;
        for j in (0..stack_size).rev() {
            if let Some(macro_obj) = self.scope_stack[j].get(&name) {
                macro_obj_opt = Some(macro_obj.clone());
                // 再在 shadow_stack 中检查是否被遮蔽
                for k in (j..stack_size).rev() {
                    if self.shadow_stack[k].contains(&name) {
                        macro_obj_opt = None;
                        break;
                    }
                }
                break;
            }
        }
        // 是宏调用
        if let Some(macro_obj) = macro_obj_opt {
            // 首先考虑常量宏
            if macro_obj.params.is_empty() {
                self.parse_index = current_parse_index; // 回到 ident 位置
                self.output.push_str(&macro_obj.expand(&[]));
                self.skip(); // 跳过 ident
                self.consume_whitespace();
                return;
            }
            // 否则是函数宏调用，检查下一个非空白 token 是否为 '('
            self.skip();
            self.consume_whitespace();
            self.check_eof();
            if self.current_kind() != &Token::LParen {
                log_error!(
                    "{}: macro {} expects {} arguments, but got 0",
                    self.current_pos(),
                    name,
                    macro_obj.params.len()
                );
            }
            // 解析参数
            let mut args: Vec<String> = Vec::new();
            let mut current_arg = String::new();
            let mut paren_level = 0;
            while !self.finished() {
                let tk = &self.tokens[self.parse_index];
                if tk.kind == Token::RParen && paren_level == 0 {
                    if !current_arg.trim().is_empty() {
                        args.push(current_arg.trim().to_string());
                    }
                    self.skip(); // 跳过 RParen
                    break;
                } else if tk.kind == Token::Comma && paren_level == 0 {
                    args.push(current_arg.trim().to_string());
                    current_arg.clear();
                    self.skip(); // 跳过 Comma
                } else {
                    if tk.kind == Token::LParen {
                        paren_level += 1;
                    } else if tk.kind == Token::RParen {
                        paren_level -= 1;
                    }
                    current_arg.push_str(&tk.text);
                    self.skip();
                }
            }
            // 参数数量校验
            if args.len() != macro_obj.params.len() {
                log_error!(
                    "{}: macro {} expects {} arguments, but got {}",
                    self.current_pos(),
                    name,
                    macro_obj.params.len(),
                    args.len()
                );
            }
            // 展开宏
            let expanded = macro_obj.expand(&args);
            self.output.push_str(&expanded);
            self.skip(); // 跳过 LParen
        } else {
            // 不是宏调用
            self.parse_index = current_parse_index; // 回到 ident 位置
            self.consume(); // ident
            self.consume_whitespace();
        }
    }

    pub fn expand(&mut self, level: u8, level_map: &HashMap<String, u8>) {
        self.reset_parse_index();
        while !self.finished() {
            match self.current_kind() {
                // 如果是局部函数，会优先被 LocalKw 捕获，因此这里的 FunctionKw 一定是全局函数
                Token::FunctionKw => {
                    self.parse_function_args(true);
                }
                Token::ForKw => {
                    self.parse_for_variables();
                }
                Token::DoKw | Token::ThenKw | Token::RepeatKw => {
                    // 进入新作用域
                    self.enter_scope();
                    self.consume();
                }
                Token::EndKw | Token::UntilKw => {
                    // 退出作用域
                    self.exit_scope();
                    self.consume();
                }
                Token::ElseKw | Token::ElseIfKw => {
                    // 退出并进入新的作用域
                    self.exit_scope();
                    self.enter_scope();
                    self.consume();
                }
                // 还是要检查宏，如果是局部宏，则允许它在当前作用域生效。
                Token::MacroComment => {
                    self.parse_local_macro();
                }
                Token::AliasComment => {
                    self.parse_alias();
                }
                Token::LocalKw => {
                    self.parse_local();
                }
                Token::Ident => {
                    self.parse_ident();
                }
                _ => {
                    self.consume();
                }
            }
        }
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
