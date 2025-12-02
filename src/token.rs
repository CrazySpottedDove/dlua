use logos::Logos;

#[derive(Logos, Debug, PartialEq, Clone)]
pub enum Token {
    // 指令注释
    #[regex(r"--\s*@macro[^\n]*", priority = 40)]
    MacroComment,
    #[regex(r"--\s*@if[^\n]*", priority = 39)]
    IfComment,
    #[regex(r"--\s*@endif[^\n]*", priority = 38)]
    EndIfComment,

    // 普通注释
    #[regex(r"--[^\n]*", priority = 20)]
    Comment,

    // 结构关键词（用于作用域划分）
    #[token("function", priority = 30)]
    FunctionKw,
    #[token("local", priority = 30)]
    LocalKw,
    #[token("do", priority = 30)]
    DoKw,
    #[token("end", priority = 30)]
    EndKw,
    #[token("if", priority = 30)]
    IfKw,
    #[token("then", priority = 30)]
    ThenKw,
    #[token("else", priority = 30)]
    ElseKw,
    #[token("elseif", priority = 30)]
    ElseIfKw,
    #[token("for", priority = 30)]
    ForKw,
    #[token("while", priority = 30)]
    WhileKw,
    #[token("repeat", priority = 30)]
    RepeatKw,
    #[token("until", priority = 30)]
    UntilKw,
    #[token("return", priority = 30)]
    ReturnKw,
    // require
    #[token("require", priority = 25)]
    Require,

    // 括号与分隔
    #[token("(", priority = 10)]
    LParen,
    #[token(")", priority = 10)]
    RParen,
    #[token(",", priority = 10)]
    Comma,

    // 标识符
    #[regex(r"[A-Za-z_][A-Za-z0-9_\.]*", priority = 5)]
    Ident,

    // 字符串（如果仍需 require 分析）
    #[regex(r#""([^"\\]|\\(\r\n|\n|.))*""#, priority = 3)]
    #[regex(r#"'([^'\\]|\\(\r\n|\n|.))*'"#, priority = 3)]
    String,

    // 赋值符号
    #[token("=", priority = 4)]
    Assign,

    // 空白
    #[regex(r"[ \t\r\n]+", priority = 2)]
    Whitespace,

    // 其它（原样透传）
    #[regex(r".", priority = 0)]
    Other,
}