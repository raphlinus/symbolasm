use crate::lex::TokBody;

#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub enum Precedence {
    Multiplication,
    Addition,
    Shift,
    BitAnd,
    BitXor,
    BitOr,
    Loosest,
}

impl Precedence {
    pub fn of(tok: &TokBody) -> Option<Self> {
        match tok {
            TokBody::Asterisk | TokBody::Slash => Some(Precedence::Multiplication),
            TokBody::Plus | TokBody::Minus => Some(Precedence::Addition),
            _ => None,
        }
    }
}
