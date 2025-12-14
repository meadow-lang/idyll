#[derive(Debug, PartialEq, Eq)]
pub enum Opcode {
    PushGlobal(String),
    PushInt(i64),
    Push(usize),
}
