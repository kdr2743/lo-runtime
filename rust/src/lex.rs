use std::{iter::Peekable, str::CharIndices};

// note: here the lexer borrows the source string as a reference to prevent needless copying of entire files
pub struct Lexer<'a> {
    source: &'a str,
    position: usize,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Lexer {
            source,
            position: 0,
            line: 1,
            column: 1,
        }     
    }
    
    pub fn pop(&mut self) -> Result<LexedToken, LexError> {

        // use a peakable iterator to go over character of source text, lexing wherever possible
        let chars_left = &mut self.source[self.position..].char_indices().peekable();
        let token = if let Some((_, c)) = chars_left.peek() {
            match c {
                'a'..='z' | 'A'..='Z' => {
                    self.lex_identifier(chars_left)
                }
                _ => {
                    Err(LexError::BadToken(c.to_string(), self.line, self.column))
                }
            }
        } else {
            Err(LexError::Empty)
        }
        unimplemented!()
    }

    pub fn peek(&self) -> Result<LexedToken, LexError> {
        unimplemented!()
    }

    fn lex_identifier(&self, chars_left: &mut Peekable<CharIndices>) -> Result<LexedToken, LexError> {
        
        let start = self.position;
        let mut end = start;
        while let Some((pos, c)) = chars_left.peek() {
            match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => {
                    end = *pos + 1;
                    chars_left.next();
                    
                }
                _ => break,
            }
        }
        // check if lexeme is a keyword
        let lexeme = &self.source[start..end];
        
        let token_type = match lexeme {
            "if" => LexTokenType::If,
            "else" => LexTokenType::Else,
            "while" => LexTokenType::While,
            "break" => LexTokenType::Break,
            "return" => LexTokenType::Return,
            "new" => LexTokenType::New,
            "instanceof" => LexTokenType::InstanceOf,
            "class" => LexTokenType::Class,
            "extends" => LexTokenType::Extends,
            "this" => LexTokenType::This,
            "super" => LexTokenType::Super,
            "null" => LexTokenType::Null,
            _ => LexTokenType::Identifier,
        };
        
        Ok(LexedToken {
            lexeme,
            line: self.line,
            column: self.column,
            token_type,
        })
    }
}

pub struct LexedToken<'a> {
    lexeme: &'a str,
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
    Empty
}

