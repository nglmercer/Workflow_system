//! A tiny recursive-descent parser for the workflow type DSL.
//!
//! It exists so the hover popup can turn a `//@T` annotation into a
//! structured field table rather than a comment-styled blob. The
//! grammar (informal):
//!
//! ```text
//! T     = atom ( '[]' )*                        // right-assoc array
//! atom  = '{' ( NAME ':' T )* '}'              // object
//!       | '(' ( NAME ':' T )* ')'              // function params or parenthesised list
//!       | NAME                                  // primitive
//! NAME  = identifier (letters, digits, `_`)
//! ```
//!
//! A `(name: T, ...) -> T` shape is recognised inside a `(...)` group
//! and returned as a [`TypeExpr::Func`]; without the arrow the same
//! shape is returned as a [`TypeExpr::Object`].

use super::model::{TypeExpr, TypeField};

#[derive(Debug)]
pub(crate) struct ParseError;

pub(crate) struct TypeParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> TypeParser<'a> {
    pub(crate) fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
            pos: 0,
        }
    }

    pub(crate) fn at_end(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(&b) = self.bytes.get(self.pos) {
            if b.is_ascii_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn eat(&mut self, b: u8) -> Result<(), ParseError> {
        self.skip_ws();
        if self.peek() == Some(b) {
            self.pos += 1;
            Ok(())
        } else {
            Err(ParseError)
        }
    }

    fn read_ident(&mut self) -> Result<String, ParseError> {
        self.skip_ws();
        let start = self.pos;
        while let Some(&b) = self.bytes.get(self.pos) {
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            return Err(ParseError);
        }
        // Safety: we just confirmed all bytes are ASCII.
        Ok(std::str::from_utf8(&self.bytes[start..self.pos])
            .unwrap_or("")
            .to_string())
    }

    pub(crate) fn parse_type_expr(&mut self) -> Result<TypeExpr, ParseError> {
        let mut atom = self.parse_atom()?;
        // `T[]` is right-associative in our DSL — `T[][]` is `Array(Array(T))`.
        loop {
            self.skip_ws();
            if self.peek() == Some(b'[') {
                self.pos += 1;
                self.eat(b']')?;
                atom = TypeExpr::Array(Box::new(atom));
            } else {
                break;
            }
        }
        Ok(atom)
    }

    fn parse_atom(&mut self) -> Result<TypeExpr, ParseError> {
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.parse_object(),
            Some(b'(') => self.parse_func_or_tuple(),
            _ => Ok(TypeExpr::Name(self.read_ident()?)),
        }
    }

    fn parse_object(&mut self) -> Result<TypeExpr, ParseError> {
        self.eat(b'{')?;
        let mut fields = Vec::new();
        self.skip_ws();
        if self.peek() != Some(b'}') {
            loop {
                self.skip_ws();
                let name = self.read_ident()?;
                self.skip_ws();
                self.eat(b':')?;
                let ty = self.parse_type_expr()?;
                fields.push(TypeField { name, ty });
                self.skip_ws();
                match self.peek() {
                    Some(b',') => {
                        self.pos += 1;
                    }
                    Some(b'}') => break,
                    _ => return Err(ParseError),
                }
            }
        }
        self.eat(b'}')?;
        Ok(TypeExpr::Object(fields))
    }

    fn parse_func_or_tuple(&mut self) -> Result<TypeExpr, ParseError> {
        self.eat(b'(')?;
        let mut params = Vec::new();
        self.skip_ws();
        if self.peek() != Some(b')') {
            loop {
                self.skip_ws();
                let name = self.read_ident()?;
                self.skip_ws();
                self.eat(b':')?;
                let ty = self.parse_type_expr()?;
                params.push(TypeField { name, ty });
                self.skip_ws();
                match self.peek() {
                    Some(b',') => {
                        self.pos += 1;
                    }
                    Some(b')') => break,
                    _ => return Err(ParseError),
                }
            }
        }
        self.eat(b')')?;
        self.skip_ws();
        // Function sig: `(name: T, ...) -> T`.
        if self.peek() == Some(b'-') && self.bytes.get(self.pos + 1) == Some(&b'>') {
            self.pos += 2;
            let ret = self.parse_type_expr()?;
            return Ok(TypeExpr::Func {
                params,
                ret: Box::new(ret),
            });
        }
        // Otherwise treat a parenthesised type-list as a record.
        Ok(TypeExpr::Object(params))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ty(s: &str) -> TypeExpr {
        let mut p = TypeParser::new(s);
        let ty = p
            .parse_type_expr()
            .unwrap_or_else(|_| panic!("parse failed for {s}"));
        assert!(p.at_end(), "trailing input for {s}");
        ty
    }

    #[test]
    fn parses_primitives() {
        assert_eq!(parse_ty("number"), TypeExpr::Name("number".into()));
        assert_eq!(parse_ty("string"), TypeExpr::Name("string".into()));
        assert_eq!(parse_ty("any"), TypeExpr::Name("any".into()));
    }

    #[test]
    fn parses_arrays_right_associative() {
        assert_eq!(
            parse_ty("number[]"),
            TypeExpr::Array(Box::new(TypeExpr::Name("number".into())))
        );
        assert_eq!(
            parse_ty("number[][]"),
            TypeExpr::Array(Box::new(TypeExpr::Array(Box::new(
                TypeExpr::Name("number".into())
            ))))
        );
    }

    #[test]
    fn parses_objects() {
        let ty = parse_ty("{ id: number, name: string }");
        match ty {
            TypeExpr::Object(fields) => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name, "id");
                assert_eq!(fields[0].ty, TypeExpr::Name("number".into()));
                assert_eq!(fields[1].name, "name");
                assert_eq!(fields[1].ty, TypeExpr::Name("string".into()));
            }
            _ => panic!("expected Object"),
        }
    }

    #[test]
    fn parses_nested_object_in_array() {
        let ty = parse_ty(
            "{ id: number, name: string, orders: { id: number, total: number }[] }[]",
        );
        match ty {
            TypeExpr::Array(inner) => match *inner {
                TypeExpr::Object(fields) => {
                    assert_eq!(fields.len(), 3);
                    assert_eq!(fields[0].name, "id");
                    assert_eq!(fields[2].name, "orders");
                    match &fields[2].ty {
                        TypeExpr::Array(o2) => match o2.as_ref() {
                            TypeExpr::Object(of) => {
                                assert_eq!(of.len(), 2);
                                assert_eq!(of[0].name, "id");
                                assert_eq!(of[1].name, "total");
                            }
                            _ => panic!("nested object expected"),
                        },
                        _ => panic!("orders should be an array"),
                    }
                }
                _ => panic!("inner should be an object"),
            },
            _ => panic!("outer should be an array"),
        }
    }

    #[test]
    fn parses_function_signatures() {
        let ty = parse_ty("(x: number, y: number) -> number");
        match ty {
            TypeExpr::Func { params, ret } => {
                assert_eq!(params.len(), 2);
                assert_eq!(params[0].name, "x");
                assert_eq!(*ret, TypeExpr::Name("number".into()));
            }
            _ => panic!("expected Func"),
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(TypeParser::new("").parse_type_expr().is_err());
        assert!(TypeParser::new("{{").parse_type_expr().is_err());
        assert!(TypeParser::new("{ a: }").parse_type_expr().is_err());
    }
}
