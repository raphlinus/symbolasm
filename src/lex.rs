//! Tokenizer

/// Location in source file. Will expand to identify multiple files.
#[derive(Clone, Debug)]
pub struct Loc {
    offset: usize,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub loc: Loc,
    pub tok: TokBody,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokBody {
    Newline,
    Idenfifier(String),
    Number(i64),
    Comma,
    OpenParen,
    CloseParen,
    OpenBracket,
    CloseBracket,
    OpenBrace,
    CloseBrace,
    Plus,
    Asterisk,
    Minus,
    Slash,
    Equals,
    Colon,
}

pub type Error = Box<dyn std::error::Error>;

#[derive(Clone, Debug)]
pub struct TokBuf {
    pub tokens: Vec<Token>,
    pub ix: usize,
}

impl Token {
    fn new(loc: Loc, tok: TokBody) -> Self {
        Token { loc, tok }
    }
}

pub fn tokenize(src: &str) -> Result<TokBuf, Error> {
    let mut tokens = vec![];
    let mut ix = 0;
    while ix < src.len() {
        let c = src[ix..].chars().next().unwrap();
        let len = c.len_utf8();
        let loc = Loc { offset: ix };
        match c {
            ' ' => (),
            '\n' => tokens.push(Token::new(loc, TokBody::Newline)),
            '(' => tokens.push(Token::new(loc, TokBody::OpenParen)),
            ')' => tokens.push(Token::new(loc, TokBody::CloseParen)),
            '{' => tokens.push(Token::new(loc, TokBody::OpenBrace)),
            '}' => tokens.push(Token::new(loc, TokBody::CloseBrace)),
            '[' => tokens.push(Token::new(loc, TokBody::OpenBracket)),
            ']' => tokens.push(Token::new(loc, TokBody::CloseBracket)),
            ',' => tokens.push(Token::new(loc, TokBody::Comma)),
            '=' => tokens.push(Token::new(loc, TokBody::Equals)),
            ':' => tokens.push(Token::new(loc, TokBody::Colon)),
            c if c.is_ascii_alphabetic() || c == '_' => {
                let mut end = ix + len;
                while end < src.len() {
                    let c1 = src[end..].chars().next().unwrap();
                    if !(c1.is_ascii_alphanumeric() || c1 == '_') {
                        break;
                    }
                    end += c1.len_utf8();
                }
                tokens.push(Token::new(loc, TokBody::Idenfifier(src[ix..end].into())));
                ix = end;
                continue;
            }
            _ => return Err(format!("unknown char {c}").into()),
        }
        ix += len;
    }
    let ix = 0;
    Ok(TokBuf { tokens, ix })
}

impl TokBody {
    pub fn match_str(&self, other: &str) -> bool {
        if let TokBody::Idenfifier(s) = self {
            s == other
        } else {
            false
        }
    }
}

impl Token {
    pub fn match_str(&self, other: &str) -> bool {
        self.tok.match_str(other)
    }
}

impl TokBuf {
    pub fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.ix)
    }

    pub fn next(&mut self) -> Option<&Token> {
        if let Some(tok) = self.tokens.get(self.ix) {
            self.ix += 1;
            Some(tok)
        } else {
            None
        }
    }

    pub fn expect(&mut self, expected: &TokBody) -> Result<(), Error> {
        if let Some(tok) = self.peek() {
            if tok.tok != *expected {
                return Err(format!("expected {expected:?} got {tok:?}").into());
            }
            self.ix += 1;
            Ok(())
        } else {
            Err("unexpected end".into())
        }
    }

    pub fn expect_opt(&mut self, expected: &TokBody) -> bool {
        if let Some(tok) = self.peek() {
            if tok.tok != *expected {
                return false;
            }
            self.ix += 1;
            true
        } else {
            false
        }
    }

    pub fn eat_newlines(&mut self) {
        while self.expect_opt(&TokBody::Newline) {}
    }
}
