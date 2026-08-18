//! Recursive descent over the M1 subset. Tokens in, AST out.

use fortress_ast::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    UnexpectedToken { span: Span, expected: &'static str },
    UnexpectedEndOfInput { expected: &'static str },
}
