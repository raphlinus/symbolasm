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
    Period,
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
    PlusEquals,
    MinusEquals,
    AsteriskEquals,
    At,
    Octothorpe,
    Ampersand,
    Pipe,
    Caret,
    Exclamation,
    Less,
    LessLess,
    Greater,
    GreaterGreater,
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

    pub fn as_ident(&self) -> Option<&str> {
        if let TokBody::Idenfifier(id) = &self.tok {
            Some(id)
        } else {
            None
        }
    }

    pub fn match_str(&self, other: &str) -> bool {
        self.tok.match_str(other)
    }

    pub fn is_ident(&self) -> bool {
        matches!(self.tok, TokBody::Idenfifier(_))
    }
}

pub fn tokenize(src: &str) -> Result<TokBuf, Error> {
    let mut tokens = vec![];
    let mut ix = 0;
    while ix < src.len() {
        let c = src[ix..].chars().next().unwrap();
        let mut len = c.len_utf8();
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
            '.' => tokens.push(Token::new(loc, TokBody::Period)),
            '=' => tokens.push(Token::new(loc, TokBody::Equals)),
            ':' => tokens.push(Token::new(loc, TokBody::Colon)),
            '+' => {
                if src.as_bytes().get(ix + 1) == Some(&b'=') {
                    tokens.push(Token::new(loc, TokBody::PlusEquals));
                    len += 1;
                } else {
                    tokens.push(Token::new(loc, TokBody::Plus));
                }
            }
            '-' => {
                if src.as_bytes().get(ix + 1) == Some(&b'=') {
                    tokens.push(Token::new(loc, TokBody::MinusEquals));
                    len += 1;
                } else {
                    tokens.push(Token::new(loc, TokBody::Minus));
                }
            }
            '*' => {
                if src.as_bytes().get(ix + 1) == Some(&b'=') {
                    tokens.push(Token::new(loc, TokBody::AsteriskEquals));
                    len += 1;
                } else {
                    tokens.push(Token::new(loc, TokBody::Asterisk));
                }
            }
            '/' => tokens.push(Token::new(loc, TokBody::Slash)),
            '@' => tokens.push(Token::new(loc, TokBody::At)),
            '#' => tokens.push(Token::new(loc, TokBody::Octothorpe)),
            '&' => tokens.push(Token::new(loc, TokBody::Ampersand)),
            '|' => tokens.push(Token::new(loc, TokBody::Pipe)),
            '^' => tokens.push(Token::new(loc, TokBody::Caret)),
            '!' => tokens.push(Token::new(loc, TokBody::Exclamation)),
            '<' => {
                if src.as_bytes().get(ix + 1) == Some(&b'<') {
                    tokens.push(Token::new(loc, TokBody::LessLess));
                    len += 1;
                } else {
                    tokens.push(Token::new(loc, TokBody::Less));
                }
            }
            '>' => {
                if src.as_bytes().get(ix + 1) == Some(&b'>') {
                    tokens.push(Token::new(loc, TokBody::GreaterGreater));
                    len += 1;
                } else {
                    tokens.push(Token::new(loc, TokBody::Greater));
                }
            }
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
            c if c.is_ascii_digit() => {
                let mut val = (c as u8 - b'0') as i64;
                let mut end = ix + 1;
                if val == 0 && end < src.len() && src.as_bytes()[end] == b'x' {
                    end += 1;
                    while end < src.len() {
                        let c1 = src.as_bytes()[end];
                        if let Some(digit) = (c1 as char).to_digit(16) {
                            val = val * 16 + digit as i64;
                        } else if c1 != b'_' {
                            break;
                        }
                        end += 1;
                    }
                    // TODO: error if end == ix + 2; bare "0x"
                }
                while end < src.len() {
                    let c1 = src.as_bytes()[end];
                    if c1.is_ascii_digit() {
                        val = (val * 10) + (c1 - b'0') as i64;
                    } else if c1.is_ascii_alphabetic() {
                        Err("trailing alphabetic in number")?;
                    } else if c1 != b'_' {
                        break;
                    }
                    end += 1;
                }
                tokens.push(Token::new(loc, TokBody::Number(val)));
                ix = end;
                continue;
            }
            _ => Err(format!("unknown char {c}"))?,
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

    pub fn is_assign_op(&self) -> bool {
        matches!(
            self,
            TokBody::Equals | TokBody::PlusEquals | TokBody::MinusEquals | TokBody::AsteriskEquals
        )
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

    pub fn back_one(&mut self) {
        self.ix -= 1;
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

    pub fn save(&self) -> usize {
        self.ix
    }

    pub fn restore(&mut self, ix: usize) {
        self.ix = ix;
    }
}
