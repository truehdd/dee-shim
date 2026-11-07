//! Inline detour engine, x86_64. OS-independent: it only computes bytes and calls the platform
//! layer to write them. Every DEE build in scope is x86_64 (the macOS one too), so a single
//! encoding covers all three OSes; an arm64 target would need a sibling encoder here.
//!
//! A detour steals the first `steal` bytes of a function (which must end on an instruction
//! boundary and be position independent: no RIP-relative operand and no relative branch), moves
//! them into a trampoline followed by a jump back to `target+steal`, and overwrites the entry
//! with an absolute jump to the hook. The hook calls the trampoline to run the original. `steal`
//! must be >= 12.
//!
//! Two jump encodings, chosen by what each must preserve:
//!
//! - **Entry patch**: `movabs rax, hook; jmp rax` (12 bytes). It clobbers rax, which is safe at a
//!   function entry: rax is never an argument register (Win64 passes in rcx/rdx/r8/r9; the SysV
//!   non-variadic call leaves al = 0, dead), and the hooked function's own first instruction
//!   defines rax before reading it. Keeping this at 12 bytes preserves the `steal >= 12` contract.
//!
//! - **Trampoline jump-back**: `jmp qword ptr [rip+0]; .qword target+steal` (14 bytes, register
//!   free). This runs *after* the stolen prologue, so it must not disturb what the prologue left in
//!   registers. A rax-clobbering jump here would crash any MSVC prologue that opens
//!   `mov rax, rsp` and then uses rax as its frame base past the steal boundary
//!   (`mov [rax+10h], rbx`, the home-space save).

use crate::platform;

/// `movabs rax, imm64; jmp rax` (12 bytes). Clobbers rax; used only for the entry patch.
fn abs_jmp(imm: usize) -> [u8; 12] {
    let mut b = [0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xE0];
    b[2..10].copy_from_slice(&(imm as u64).to_le_bytes());
    b
}

/// `jmp qword ptr [rip+0]; .qword imm` (14 bytes). Register free (the target is an inline literal
/// read RIP-relative, so it is position independent); used for the trampoline jump-back.
fn abs_jmp_indirect(imm: usize) -> [u8; 14] {
    let mut b = [0xFF, 0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    b[6..14].copy_from_slice(&(imm as u64).to_le_bytes());
    b
}

/// Install a detour at runtime address `target`. Returns a callable trampoline address that
/// runs the stolen prologue and continues into the original, or None if the code could not be
/// made writable.
pub fn install(target: usize, steal: usize, hook: usize) -> Option<usize> {
    assert!(steal >= 12, "steal {steal} < 12");
    let body = unsafe { std::slice::from_raw_parts(target as *const u8, steal) };
    install_body(target, steal, hook, body)
}

fn relocate_call(target: usize, body: &[u8], call_offset: usize) -> Option<Vec<u8>> {
    let end = call_offset.checked_add(5)?;
    let call = body.get(call_offset..end)?;
    if call[0] != 0xE8 {
        return None;
    }
    let disp = i32::from_le_bytes(call[1..].try_into().ok()?);
    let callee = target.wrapping_add(end).wrapping_add_signed(disp as isize);
    let mut relocated = body[..call_offset].to_vec();
    // Indirect call, then jump over its address literal on return. Keep rax for _chkstk.
    relocated.extend_from_slice(&[0xFF, 0x15, 2, 0, 0, 0, 0xEB, 8]);
    relocated.extend_from_slice(&(callee as u64).to_le_bytes());
    relocated.extend_from_slice(&body[end..]);
    Some(relocated)
}

/// Relocate one profiled rel32 call; all other stolen instructions must be position independent.
pub fn install_call(target: usize, steal: usize, call_offset: usize, hook: usize) -> Option<usize> {
    if steal < 12 {
        return None;
    }
    let body = unsafe { std::slice::from_raw_parts(target as *const u8, steal) };
    let relocated = relocate_call(target, body, call_offset)?;
    install_body(target, steal, hook, &relocated)
}

fn install_body(target: usize, steal: usize, hook: usize, body: &[u8]) -> Option<usize> {
    let tramp = platform::alloc_rwx(body.len() + 14);
    if tramp.is_null() {
        return None;
    }
    unsafe {
        // trampoline = [stolen bytes] [jmp qword ptr [rip+0]; .qword target+steal]
        std::ptr::copy_nonoverlapping(body.as_ptr(), tramp, body.len());
        let back = abs_jmp_indirect(target + steal);
        std::ptr::copy_nonoverlapping(back.as_ptr(), tramp.add(body.len()), 14);
    }
    platform::flush_icache(tramp as usize, body.len() + 14);

    // entry = movabs rax, hook; jmp rax, NOP-padded to the stolen length.
    let mut patch = vec![0x90u8; steal];
    patch[..12].copy_from_slice(&abs_jmp(hook));
    if !platform::write_code(target, &patch) {
        return None;
    }
    Some(tramp as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relocated_call_preserves_target_and_continuation() {
        for disp in [-4096i32, 4096] {
            let mut body = vec![0x50, 0xE8];
            body.extend_from_slice(&disp.to_le_bytes());
            body.push(0x58);
            let relocated = relocate_call(0x100000000, &body, 1).unwrap();
            assert_eq!(relocated[0], 0x50);
            assert_eq!(&relocated[1..9], &[0xFF, 0x15, 2, 0, 0, 0, 0xEB, 8]);
            let target = u64::from_le_bytes(relocated[9..17].try_into().unwrap());
            assert_eq!(target, (0x100000006i64 + disp as i64) as u64);
            assert_eq!(relocated[17], 0x58);
        }
        assert!(relocate_call(0, &[0x90; 16], 11).is_none());
        assert!(relocate_call(0, &[0xE8; 4], 0).is_none());
    }

    #[test]
    fn trampoline_jmp_back_is_register_free() {
        let b = abs_jmp_indirect(0x1122334455667788);
        // jmp qword ptr [rip+0]; the disp32 is 0 so the literal sits right after the instruction.
        assert_eq!(&b[..6], &[0xFF, 0x25, 0x00, 0x00, 0x00, 0x00]);
        assert_eq!(&b[6..], &0x1122334455667788u64.to_le_bytes());
        // No register operand bytes anywhere: nothing for a stolen `mov rax, rsp` to lose.
    }
}
