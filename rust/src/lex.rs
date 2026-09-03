pub struct LexedToken {
    lexeme: String,
    line: usize,
    column: usize,
    token_type: LexTokenType,
}

pub enum LexTokenType {
    Literal(LiteralType),
    Identifier,

    Binop(BinopType),
    Unop(UnopType),
    Type(BuiltInType),

    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,

    Comma,
    Period,
    Semicolon,
    Equals, // note: equals is used as a binop and for assignment. Parser must decide which
    QuestionMark,
    Colon,

    New,
    InstanceOf,
    Class,
    Extends,
    This,
    Super,
    Null,

    If,
    Else,
    While,
    Break,
    Return,
    Eof,
}

pub enum LiteralType {
    IntLiteral(i32),
    BoolLiteral(bool),
    StringLiteral, // actual string is stored in lexeme
}

// these are the keywords bool, int, String, etc.
pub enum BuiltInType {
    IntType,
    StringType,
    VoidType,
    BoolType,
}

pub enum UnopType {
    BitNot, // ~
    Not,    // !
}

pub enum BinopType {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulus,
    BitAnd, // &
    BitOr,  // |
    LessThan,
    GreaterThan,
}

// TODO define all lexer errors based on spec`
pub enum LexError {
    BadToken(String, usize, usize), // (lexeme, line, column)
}

// TODO add necessary state like current position
pub struct Lexer;

impl Lexer {
    pub fn pop(&mut self) -> Result<LexedToken, LexError> {
        unimplemented!()
    }

    pub fn peek(&self) -> Result<LexedToken, LexError> {
        unimplemented!()
    }
}
