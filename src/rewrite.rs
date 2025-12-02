use std::collections::HashMap;

use crate::token::Token;
use logos::{Logos, Lexer};

#[derive(Debug, Clone)]
pub struct Tok {
    pub kind: Token,
    pub text: String,
}

pub struct Rewriter{
    global_macros: HashMap<String, String>,
    local_macros: HashMap<String, String>
}

impl Rewriter{

}

fn lex_with_text(mut lex: Lexer<Token>) -> Vec<Tok> {
    let src = lex.source();
    let mut toks = Vec::new();
    while let Some(kind) = lex.next() {
        let span = lex.span();
        toks.push(Tok {
            kind: kind.unwrap_or(Token::Unknown),
            text: src[span.start..span.end].to_string(),
        });
    }
    toks
}

pub fn rewrite_tokens(tokens: &[Tok]) -> String {
    let mut out = String::new();
    let mut i: usize = 0;

    while i < tokens.len() {
        match tokens[i].kind {
            Token::Macro =>{
                if i > 0 {
                    let tok = &tokens[i];
                    if tok.kind == Token::Local{

                    }

                }
            },
            Token::Ident => {
                // 记录标识符文本
                let left = tokens[i].text.clone();

                // 吞掉标识符后的空白，先缓存在 ws_after_ident，
                // 若不是增量赋值，再把它们吐回去，避免丢空白
                let mut j = i + 1;
                let mut ws_after_ident = String::new();
                while j < tokens.len() && matches!(tokens[j].kind, Token::Whitespace) {
                    ws_after_ident.push_str(&tokens[j].text);
                    j += 1;
                }

                // 检查是否是 += / -= / *= / /=
                let op_sym = match tokens.get(j).map(|t| &t.kind) {
                    Some(Token::PlusEq) => "+",
                    Some(Token::MinusEq) => "-",
                    Some(Token::StarEq) => "*",
                    Some(Token::SlashEq) => "/",
                    _ => "",
                };

                if op_sym.is_empty() {
                    // 不是增量赋值，原样输出 Ident 和跟随空白
                    out.push_str(&left);
                    out.push_str(&ws_after_ident);
                    i = j;
                    continue;
                }

                // 收集 RHS（从 j+1 开始），直到分号或换行或单行注释前
                let mut k = j + 1;

                // 跳过运算符后的前导空白（若遇换行，则放弃重写）
                while k < tokens.len() && matches!(tokens[k].kind, Token::Whitespace) {
                    if tokens[k].text.contains('\n') || tokens[k].text.contains('\r') {
                        // a += \n 这种，不重写，按原样输出
                        out.push_str(&left);
                        out.push_str(&ws_after_ident);
                        out.push_str(&tokens[j].text); // 运算符文本
                        i = j + 1;
                        continue;
                    }
                    k += 1;
                }

                let mut rhs = String::new();
                let mut m = k;
                while m < tokens.len() {
                    match tokens[m].kind {
                        Token::Semicolon => break,
                        Token::SingleLineComment => break,
                        Token::Whitespace => {
                            if tokens[m].text.contains('\n') || tokens[m].text.contains('\r') {
                                break;
                            } else {
                                rhs.push_str(&tokens[m].text);
                            }
                        }
                        _ => rhs.push_str(&tokens[m].text),
                    }
                    m += 1;
                }
                let rhs_trim = rhs.trim();

                // 输出标准 Lua：left = left op (rhs)
                out.push_str(&format!("{l} = {l} {op} ({r})", l = left, op = op_sym, r = rhs_trim));

                // 若遇到分号，保留分号
                if m < tokens.len() && matches!(tokens[m].kind, Token::Semicolon) {
                    out.push_str(&tokens[m].text);
                    m += 1;
                }

                // 跳过本条语句
                i = m;
            }

            // 其它 token：原样输出
            _ => {
                out.push_str(&tokens[i].text);
                i += 1;
            }
        }
    }

    out
}

pub fn expand_sugar_in_source(lexer: Lexer<Token>) -> String {
    let toks = lex_with_text(lexer);
    rewrite_tokens(&toks)
}