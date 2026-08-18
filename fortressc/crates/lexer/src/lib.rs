//! Tokenizer plus the newline and layout layer.
//!
//! Whitespace is not uniformly ignorable in Fortress: juxtaposition carries
//! meaning and newlines terminate statements. The token set and the layout
//! state machine are defined in plan steps 2 and 3.

use fortress_ast::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexError {
    UnrecognizedCharacter { span: Span },
    UnterminatedStringLiteral { span: Span },
    UnterminatedComment { span: Span },
}

impl LexError {
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::UnrecognizedCharacter { span }
            | Self::UnterminatedStringLiteral { span }
            | Self::UnterminatedComment { span } => *span,
        }
    }
}
