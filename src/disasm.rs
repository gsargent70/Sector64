//! 6502 disassembly: machine code listed one instruction per line, the way a
//! machine-code monitor shows it.
//!
//! All 256 opcodes decode. The 151 documented ones use their usual mnemonics;
//! the NMOS 6510's undocumented ones (which C64 code does use) use the common
//! community names (`LAX`, `SAX`, `DCP`, `JAM`…) and are flagged, so a viewer
//! can mark them: in a listing they are often a sign the bytes are data.

/// How an instruction finds its operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// No operand (`RTS`).
    Implied,
    /// The accumulator (`ASL A`).
    Accumulator,
    /// `#$nn`
    Immediate,
    /// `$nn`
    ZeroPage,
    /// `$nn,X`
    ZeroPageX,
    /// `$nn,Y`
    ZeroPageY,
    /// `$nnnn`
    Absolute,
    /// `$nnnn,X`
    AbsoluteX,
    /// `$nnnn,Y`
    AbsoluteY,
    /// `($nnnn)`, only `JMP`.
    Indirect,
    /// `($nn,X)`
    IndirectX,
    /// `($nn),Y`
    IndirectY,
    /// A branch: a signed offset from the next instruction.
    Relative,
}

impl Mode {
    /// Instruction length in bytes, opcode included.
    pub fn bytes(self) -> usize {
        match self {
            Mode::Implied | Mode::Accumulator => 1,
            Mode::Absolute | Mode::AbsoluteX | Mode::AbsoluteY | Mode::Indirect => 3,
            _ => 2,
        }
    }
}

/// One opcode's meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opcode {
    pub mnemonic: &'static str,
    pub mode: Mode,
    /// Not one of the 151 documented opcodes.
    pub undocumented: bool,
}

/// The opcode matrix, row = high nibble, column = low nibble. Each entry is
/// mnemonic and mode; a leading `*` marks an undocumented opcode.
///
/// Modes: `imp`, `acc`, `imm`, `zp`, `zpx`, `zpy`, `abs`, `abx`, `aby`, `ind`,
/// `izx`, `izy`, `rel`.
#[rustfmt::skip]
const MATRIX: [[(&str, &str); 16]; 16] = [
    // $00
    [("BRK","imp"),("ORA","izx"),("*JAM","imp"),("*SLO","izx"),("*NOP","zp"),("ORA","zp"),("ASL","zp"),("*SLO","zp"),
     ("PHP","imp"),("ORA","imm"),("ASL","acc"),("*ANC","imm"),("*NOP","abs"),("ORA","abs"),("ASL","abs"),("*SLO","abs")],
    // $10
    [("BPL","rel"),("ORA","izy"),("*JAM","imp"),("*SLO","izy"),("*NOP","zpx"),("ORA","zpx"),("ASL","zpx"),("*SLO","zpx"),
     ("CLC","imp"),("ORA","aby"),("*NOP","imp"),("*SLO","aby"),("*NOP","abx"),("ORA","abx"),("ASL","abx"),("*SLO","abx")],
    // $20
    [("JSR","abs"),("AND","izx"),("*JAM","imp"),("*RLA","izx"),("BIT","zp"),("AND","zp"),("ROL","zp"),("*RLA","zp"),
     ("PLP","imp"),("AND","imm"),("ROL","acc"),("*ANC","imm"),("BIT","abs"),("AND","abs"),("ROL","abs"),("*RLA","abs")],
    // $30
    [("BMI","rel"),("AND","izy"),("*JAM","imp"),("*RLA","izy"),("*NOP","zpx"),("AND","zpx"),("ROL","zpx"),("*RLA","zpx"),
     ("SEC","imp"),("AND","aby"),("*NOP","imp"),("*RLA","aby"),("*NOP","abx"),("AND","abx"),("ROL","abx"),("*RLA","abx")],
    // $40
    [("RTI","imp"),("EOR","izx"),("*JAM","imp"),("*SRE","izx"),("*NOP","zp"),("EOR","zp"),("LSR","zp"),("*SRE","zp"),
     ("PHA","imp"),("EOR","imm"),("LSR","acc"),("*ALR","imm"),("JMP","abs"),("EOR","abs"),("LSR","abs"),("*SRE","abs")],
    // $50
    [("BVC","rel"),("EOR","izy"),("*JAM","imp"),("*SRE","izy"),("*NOP","zpx"),("EOR","zpx"),("LSR","zpx"),("*SRE","zpx"),
     ("CLI","imp"),("EOR","aby"),("*NOP","imp"),("*SRE","aby"),("*NOP","abx"),("EOR","abx"),("LSR","abx"),("*SRE","abx")],
    // $60
    [("RTS","imp"),("ADC","izx"),("*JAM","imp"),("*RRA","izx"),("*NOP","zp"),("ADC","zp"),("ROR","zp"),("*RRA","zp"),
     ("PLA","imp"),("ADC","imm"),("ROR","acc"),("*ARR","imm"),("JMP","ind"),("ADC","abs"),("ROR","abs"),("*RRA","abs")],
    // $70
    [("BVS","rel"),("ADC","izy"),("*JAM","imp"),("*RRA","izy"),("*NOP","zpx"),("ADC","zpx"),("ROR","zpx"),("*RRA","zpx"),
     ("SEI","imp"),("ADC","aby"),("*NOP","imp"),("*RRA","aby"),("*NOP","abx"),("ADC","abx"),("ROR","abx"),("*RRA","abx")],
    // $80
    [("*NOP","imm"),("STA","izx"),("*NOP","imm"),("*SAX","izx"),("STY","zp"),("STA","zp"),("STX","zp"),("*SAX","zp"),
     ("DEY","imp"),("*NOP","imm"),("TXA","imp"),("*ANE","imm"),("STY","abs"),("STA","abs"),("STX","abs"),("*SAX","abs")],
    // $90
    [("BCC","rel"),("STA","izy"),("*JAM","imp"),("*SHA","izy"),("STY","zpx"),("STA","zpx"),("STX","zpy"),("*SAX","zpy"),
     ("TYA","imp"),("STA","aby"),("TXS","imp"),("*TAS","aby"),("*SHY","abx"),("STA","abx"),("*SHX","aby"),("*SHA","aby")],
    // $A0
    [("LDY","imm"),("LDA","izx"),("LDX","imm"),("*LAX","izx"),("LDY","zp"),("LDA","zp"),("LDX","zp"),("*LAX","zp"),
     ("TAY","imp"),("LDA","imm"),("TAX","imp"),("*LXA","imm"),("LDY","abs"),("LDA","abs"),("LDX","abs"),("*LAX","abs")],
    // $B0
    [("BCS","rel"),("LDA","izy"),("*JAM","imp"),("*LAX","izy"),("LDY","zpx"),("LDA","zpx"),("LDX","zpy"),("*LAX","zpy"),
     ("CLV","imp"),("LDA","aby"),("TSX","imp"),("*LAS","aby"),("LDY","abx"),("LDA","abx"),("LDX","aby"),("*LAX","aby")],
    // $C0
    [("CPY","imm"),("CMP","izx"),("*NOP","imm"),("*DCP","izx"),("CPY","zp"),("CMP","zp"),("DEC","zp"),("*DCP","zp"),
     ("INY","imp"),("CMP","imm"),("DEX","imp"),("*SBX","imm"),("CPY","abs"),("CMP","abs"),("DEC","abs"),("*DCP","abs")],
    // $D0
    [("BNE","rel"),("CMP","izy"),("*JAM","imp"),("*DCP","izy"),("*NOP","zpx"),("CMP","zpx"),("DEC","zpx"),("*DCP","zpx"),
     ("CLD","imp"),("CMP","aby"),("*NOP","imp"),("*DCP","aby"),("*NOP","abx"),("CMP","abx"),("DEC","abx"),("*DCP","abx")],
    // $E0
    [("CPX","imm"),("SBC","izx"),("*NOP","imm"),("*ISB","izx"),("CPX","zp"),("SBC","zp"),("INC","zp"),("*ISB","zp"),
     ("INX","imp"),("SBC","imm"),("NOP","imp"),("*USBC","imm"),("CPX","abs"),("SBC","abs"),("INC","abs"),("*ISB","abs")],
    // $F0
    [("BEQ","rel"),("SBC","izy"),("*JAM","imp"),("*ISB","izy"),("*NOP","zpx"),("SBC","zpx"),("INC","zpx"),("*ISB","zpx"),
     ("SED","imp"),("SBC","aby"),("*NOP","imp"),("*ISB","aby"),("*NOP","abx"),("SBC","abx"),("INC","abx"),("*ISB","abx")],
];

/// What opcode byte `b` means.
pub fn opcode(b: u8) -> Opcode {
    let (name, mode) = MATRIX[(b >> 4) as usize][(b & 15) as usize];
    let mode = match mode {
        "imp" => Mode::Implied,
        "acc" => Mode::Accumulator,
        "imm" => Mode::Immediate,
        "zp" => Mode::ZeroPage,
        "zpx" => Mode::ZeroPageX,
        "zpy" => Mode::ZeroPageY,
        "abs" => Mode::Absolute,
        "abx" => Mode::AbsoluteX,
        "aby" => Mode::AbsoluteY,
        "ind" => Mode::Indirect,
        "izx" => Mode::IndirectX,
        "izy" => Mode::IndirectY,
        _ => Mode::Relative,
    };
    let undocumented = name.starts_with('*');
    Opcode {
        mnemonic: name.trim_start_matches('*'),
        mode,
        undocumented,
    }
}

/// One disassembled instruction (or, at the end of the bytes, a lone byte
/// too short to be one).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// Where the instruction sits in memory.
    pub addr: u16,
    /// Its bytes: 1 to 3.
    pub bytes: Vec<u8>,
    /// Mnemonic and operand, e.g. `LDA #$00`, or `.BYTE $A9` for an
    /// instruction cut off by the end of the bytes.
    pub text: String,
    /// The opcode is undocumented.
    pub undocumented: bool,
}

impl std::fmt::Display for Line {
    /// `C000  A9 00     LDA #$00`, with a `*` before an undocumented
    /// mnemonic.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hex: Vec<String> = self.bytes.iter().map(|b| format!("{b:02X}")).collect();
        let mark = if self.undocumented { '*' } else { ' ' };
        write!(
            f,
            "{:04X}  {:<8} {mark}{}",
            self.addr,
            hex.join(" "),
            self.text
        )
    }
}

/// Disassemble `code`, which sits in memory at `origin`, into at most
/// `max_lines` lines. Addresses wrap at $FFFF as the 6502's do. Never panics.
pub fn disassemble(code: &[u8], origin: u16, max_lines: usize) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut pos = 0;
    while pos < code.len() && lines.len() < max_lines {
        let addr = origin.wrapping_add(pos as u16);
        let op = opcode(code[pos]);
        let len = op.mode.bytes();
        let Some(bytes) = code.get(pos..pos + len) else {
            lines.push(Line {
                addr,
                bytes: vec![code[pos]],
                text: format!(".BYTE ${:02X}", code[pos]),
                undocumented: false,
            });
            pos += 1;
            continue;
        };
        let b = bytes.get(1).copied().unwrap_or(0);
        let w = u16::from_le_bytes([b, bytes.get(2).copied().unwrap_or(0)]);
        let operand = match op.mode {
            Mode::Implied => String::new(),
            Mode::Accumulator => "A".into(),
            Mode::Immediate => format!("#${b:02X}"),
            Mode::ZeroPage => format!("${b:02X}"),
            Mode::ZeroPageX => format!("${b:02X},X"),
            Mode::ZeroPageY => format!("${b:02X},Y"),
            Mode::Absolute => format!("${w:04X}"),
            Mode::AbsoluteX => format!("${w:04X},X"),
            Mode::AbsoluteY => format!("${w:04X},Y"),
            Mode::Indirect => format!("(${w:04X})"),
            Mode::IndirectX => format!("(${b:02X},X)"),
            Mode::IndirectY => format!("(${b:02X}),Y"),
            Mode::Relative => {
                let target = addr.wrapping_add(2).wrapping_add(b as i8 as u16);
                format!("${target:04X}")
            }
        };
        let text = if operand.is_empty() {
            op.mnemonic.to_string()
        } else {
            format!("{} {operand}", op.mnemonic)
        };
        lines.push(Line {
            addr,
            bytes: bytes.to_vec(),
            text,
            undocumented: op.undocumented,
        });
        pos += len;
    }
    lines
}

/// Where to start disassembling a PRG (a file whose first two bytes are its
/// load address).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodeStart {
    /// The file's load address.
    pub load: u16,
    /// The address to start at.
    pub addr: u16,
    /// The file offset of that address.
    pub offset: usize,
    /// It's where a BASIC line's `SYS` jumps, not the load address.
    pub from_sys: bool,
}

/// Where a PRG's code starts: for a BASIC program whose `SYS` jumps into the
/// file, that address; otherwise the load address. `None` when the file is
/// too short to have a load address.
pub fn code_start(prg: &[u8]) -> Option<CodeStart> {
    let load = u16::from_le_bytes([*prg.first()?, *prg.get(1)?]);
    let sys = crate::basic::sys_address(prg)
        .filter(|&a| a >= load)
        .map(|a| (a, 2 + usize::from(a - load)))
        .filter(|&(_, off)| off < prg.len());
    Some(match sys {
        Some((addr, offset)) => CodeStart {
            load,
            addr,
            offset,
            from_sys: true,
        },
        None => CodeStart {
            load,
            addr: load,
            offset: 2,
            from_sys: false,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(code: &[u8], origin: u16) -> Vec<String> {
        disassemble(code, origin, usize::MAX)
            .iter()
            .map(|l| l.to_string())
            .collect()
    }

    #[test]
    fn every_addressing_mode_formats_like_a_monitor() {
        let code = [
            0xA9, 0x00, // LDA #$00
            0x0A, // ASL A
            0x6C, 0x34, 0x12, // JMP ($1234)
            0xA1, 0x10, // LDA ($10,X)
            0xB1, 0x20, // LDA ($20),Y
            0xD0, 0xFE, // BNE to itself
            0x96, 0x10, // STX $10,Y
            0xBD, 0x00, 0xD0, // LDA $D000,X
            0x60, // RTS
        ];
        assert_eq!(
            list(&code, 0xC000),
            [
                "C000  A9 00     LDA #$00",
                "C002  0A        ASL A",
                "C003  6C 34 12  JMP ($1234)",
                "C006  A1 10     LDA ($10,X)",
                "C008  B1 20     LDA ($20),Y",
                "C00A  D0 FE     BNE $C00A",
                "C00C  96 10     STX $10,Y",
                "C00E  BD 00 D0  LDA $D000,X",
                "C011  60        RTS",
            ]
        );
    }

    #[test]
    fn undocumented_opcodes_are_marked() {
        assert_eq!(
            list(&[0xA7, 0x10, 0xEB, 0x01, 0x02], 0x1000),
            [
                "1000  A7 10    *LAX $10",
                "1002  EB 01    *USBC #$01",
                "1004  02       *JAM",
            ]
        );
        let documented = (0..=255u8).filter(|&b| !opcode(b).undocumented).count();
        assert_eq!(documented, 151);
    }

    #[test]
    fn branches_reach_back_and_wrap_and_short_tails_become_bytes() {
        // A backward branch, and a forward one across $FFFF.
        assert_eq!(list(&[0x10, 0x80], 0x0810), ["0810  10 80     BPL $0792"]);
        assert_eq!(list(&[0xF0, 0x10], 0xFFF0), ["FFF0  F0 10     BEQ $0002"]);
        // A JSR cut short by the end of the file.
        assert_eq!(
            list(&[0xEA, 0x20, 0x00], 0x2000),
            [
                "2000  EA        NOP",
                "2001  20        .BYTE $20",
                "2002  00        BRK",
            ]
        );
    }

    #[test]
    fn code_starts_at_the_sys_target_or_the_load_address() {
        // 10 SYS 2061, then code at $080D.
        let mut stub = crate::basic::tests::prg(0x0801, &[(10, b"\x9e2061")]);
        assert_eq!(stub.len(), 2 + 12);
        stub.extend_from_slice(&[0xEE, 0x20, 0xD0, 0x60]);
        let s = code_start(&stub).unwrap();
        assert_eq!((s.addr, s.offset, s.from_sys), (0x080D, 14, true));
        assert_eq!(
            disassemble(&stub[s.offset..], s.addr, 9)[0].to_string(),
            "080D  EE 20 D0  INC $D020"
        );
        // A SYS past the end of the file, or plain machine code: the load
        // address.
        stub.truncate(14);
        assert_eq!(code_start(&stub).unwrap().addr, 0x0801);
        let ml = [0x00, 0xC0, 0x60];
        let s = code_start(&ml).unwrap();
        assert_eq!(
            (s.load, s.addr, s.offset, s.from_sys),
            (0xC000, 0xC000, 2, false)
        );
        assert_eq!(code_start(&[0x01]), None);
    }
}
