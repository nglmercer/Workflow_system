use eframe::egui::Color32;
use std::collections::HashSet;

#[derive(Clone, Copy)]
pub enum TokenKind {
    Keyword,
    String,
    Number,
    Comment,
    Function,
    Operator,
    Punctuation,
    Variable,
}

pub struct Token {
    pub text: String,
    pub kind: TokenKind,
}

/// Tokenize a line of code with awareness of known function names.
/// The `known_functions` parameter allows the tokenizer to recognize
/// functions registered in the dynamic FunctionRegistry.
pub fn tokenize_line(line: &str, known_functions: &HashSet<String>) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut i = 0;
    let bytes = line.as_bytes();

    while i < bytes.len() {
        let ch = bytes[i] as char;

        if ch == '/' && i + 1 < bytes.len() && bytes[i + 1] as char == '/' {
            tokens.push(Token {
                text: line[i..].to_string(),
                kind: TokenKind::Comment,
            });
            break;
        }

        if ch == '"' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] as char == '"' {
                    i += 1;
                    break;
                }
                if bytes[i] as char == '\\' {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            tokens.push(Token {
                text: line[start..i].to_string(),
                kind: TokenKind::String,
            });
            continue;
        }

        if ch.is_ascii_digit()
            || (ch == '-' && i + 1 < bytes.len() && (bytes[i + 1] as char).is_ascii_digit())
        {
            let start = i;
            if ch == '-' {
                i += 1;
            }
            while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] as char == '.' {
                i += 1;
                while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                    i += 1;
                }
            }
            tokens.push(Token {
                text: line[start..i].to_string(),
                kind: TokenKind::Number,
            });
            continue;
        }

        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            i += 1;
            while i < bytes.len()
                && ((bytes[i] as char).is_ascii_alphanumeric() || bytes[i] as char == '_')
            {
                i += 1;
            }
            let word = &line[start..i];
            let kind = match word {
                "workflow" | "fn" | "var" | "if" | "else" | "foreach" | "in" | "on" | "return"
                | "true" | "false" | "null" | "import" | "from" | "emit" => TokenKind::Keyword,
                _ => {
                    // Check if it's a known function (from registry or builtins)
                    if known_functions.contains(word) {
                        TokenKind::Function
                    } else {
                        TokenKind::Variable
                    }
                }
            };
            tokens.push(Token {
                text: word.to_string(),
                kind,
            });
            continue;
        }

        if "+-*/%=<>!&|".contains(ch) {
            let start = i;
            if i + 1 < bytes.len() {
                let two = &line[i..i + 2];
                if matches!(two, "==" | "!=" | "<=" | ">=" | "&&" | "||") {
                    i += 2;
                    tokens.push(Token {
                        text: two.to_string(),
                        kind: TokenKind::Operator,
                    });
                    continue;
                }
            }
            i += 1;
            tokens.push(Token {
                text: line[start..i].to_string(),
                kind: TokenKind::Operator,
            });
            continue;
        }

        if "(){}[],.".contains(ch) {
            tokens.push(Token {
                text: ch.to_string(),
                kind: TokenKind::Punctuation,
            });
            i += 1;
            continue;
        }

        tokens.push(Token {
            text: ch.to_string(),
            kind: TokenKind::Variable,
        });
        i += 1;
    }

    tokens
}

pub fn token_color(kind: TokenKind) -> Color32 {
    match kind {
        TokenKind::Keyword => Color32::from_rgb(200, 120, 255),
        TokenKind::String => Color32::from_rgb(180, 220, 120),
        TokenKind::Number => Color32::from_rgb(255, 180, 100),
        TokenKind::Comment => Color32::from_gray(100),
        TokenKind::Function => Color32::from_rgb(100, 200, 255),
        TokenKind::Operator => Color32::from_rgb(255, 120, 120),
        TokenKind::Punctuation => Color32::from_gray(180),
        TokenKind::Variable => Color32::WHITE,
    }
}
