// SPDX-License-Identifier: GPL-2.0-or-later
//
// bubbledit-gtk
// A tool to manipulate MAME image files of Intel's bubble memories
// Copyright (C) 2026 F. Ulivi <fulivi at big "G" mail>
//
// This program is free software; you can redistribute it and/or
// modify it under the terms of the GNU General Public License
// as published by the Free Software Foundation; either version 2
// of the License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see
// <http://www.gnu.org/licenses/>.

use core::fmt;
use rand::prelude::*;
use std::fmt::Write as _;
use std::io::prelude::*;
use std::io::{self, Error};
use std::iter::zip;

pub const BITS_PER_LOOP: usize = 4096;
pub const LOOPS_PER_QUAD: usize = 80;
pub const QUADS_PER_CH: usize = 2;
pub const CH_PER_MEM: usize = 2;
// Size of a channel bootloop
const CH_BL_SIZE: usize = LOOPS_PER_QUAD * QUADS_PER_CH;
// Size of memory bootloop
const BL_SIZE: usize = CH_BL_SIZE * CH_PER_MEM;
/// Size of encoded loop
const ENC_LOOP_SIZE: usize = BITS_PER_LOOP / 8;
pub type Address = u16;
/// Offset of PA=0 wrt the synchronization position
const SYNC_OFFSET: Address = 362;
const LOOP_IDX_MASK: usize = BITS_PER_LOOP - 1;
const BL_PRE_SYNC_BITS: usize = 3158;
const BL_SYNC_BITS: usize = 242;
const BL_PATTERN_BITS: usize = 14;
const BL_PAD_BITS: usize = 362;
/// Default offset
const DEF_OFFSET: Address = 0;
/// PA mask
const PA_MASK: Address = (BITS_PER_LOOP as Address) - 1;
/// Count of valid logical addresses
const LOG_ADDRS: Address = (BITS_PER_LOOP as Address) / 2;
/// Distance in logical addresses between consecutive physical addresses
const LA_DIST: Address = 0x589;
/// Multiplicative inverse of LA_DIST (1417) modulo 2048
const LA_DIST_INV: Address = 185;
/// Bits in a error-corrected page from a channel
const EC_BITS: usize = 270;
/// Bits in a non error-corrected page from a channel
const NO_EC_BITS: usize = 272;
/// Minimum amount of 1s in a valid channel bootloop
const MIN_ONES: usize = 136;
/// Size of a logical page with EC enabled (bytes)
const LOG_PAGE_SIZE: usize = (PAYLOAD_BITS * CH_PER_MEM) / 8;
/// Size of full memory image (bytes)
pub const IMG_SIZE: usize = LOG_PAGE_SIZE * LOG_ADDRS as usize;

enum SyncState {
    WaitPreSync,
    WaitPreSyncMinLen,
    WaitPreSyncEnd,
    WaitSyncPattern,
    ReadBL,
}

#[derive(Clone, Copy)]
pub enum Channel {
    ChannelA,
    ChannelB,
}

fn slicebool_to_arru8<const N: usize>(inp: &[bool]) -> [u8; N] {
    let mut out = [0_u8; N];
    for idx in 0..N {
        let it = &inp[idx * 8..(idx + 1) * 8];
        let byte: u8 = it.iter().fold(0, |acc, b| (acc << 1) | (*b as u8));
        out[idx] = byte;
    }
    out
}

fn to_hex_string(inp: &[u8]) -> String {
    let mut s = String::new();
    for b in inp {
        let _ = write!(&mut s, "{:02x}", b);
    }
    s
}

fn from_string<const N: usize>(s: &str) -> Option<[bool; N]> {
    if s.len() != N / 4 {
        None
    } else {
        let mut tmp = [false; N];
        let mut idx = 0;
        for c in s.chars() {
            if !c.is_ascii_hexdigit() {
                return None;
            }
            let x = u8::from_str_radix(&c.to_string(), 16);
            if let Ok(u) = x {
                for j in 0..4 {
                    if idx >= N {
                        return None;
                    }
                    tmp[idx] = (u & (8_u8 >> j)) != 0;
                    idx += 1;
                }
            } else {
                return None;
            }
        }
        Some(tmp)
    }
}

fn string_to_vecu8(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 == 0 {
        let mut out: Vec<u8> = Vec::new();
        for idx in 0..s.len() / 2 {
            let ss = &s[idx * 2..(idx + 1) * 2];
            let b = u8::from_str_radix(ss, 16);
            if let Ok(byt) = b {
                out.push(byt);
            } else {
                return None;
            }
        }
        Some(out)
    } else {
        None
    }
}

fn sliceu8_to_vecbool(s: &[u8]) -> Vec<bool> {
    let mut out: Vec<bool> = Vec::new();
    for b in s {
        let mut byt = *b;
        for _ in 0..8 {
            out.push((byt & 0x80) != 0);
            byt <<= 1;
        }
    }
    out
}

fn slicebool_to_vecu8(sl: &[bool]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut mask = 0x80_u8;
    let mut accum = 0_u8;
    for b in sl {
        if *b {
            accum |= mask;
        }
        mask >>= 1;
        if mask == 0 {
            out.push(accum);
            mask = 0x80;
            accum = 0;
        }
    }
    if mask != 0x80 {
        out.push(accum);
    }
    out
}

pub struct Bootloop {
    bl: [bool; BL_SIZE],
}

impl Default for Bootloop {
    fn default() -> Self {
        Bootloop {
            bl: [false; BL_SIZE],
        }
    }
}

impl fmt::Display for Bootloop {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.get_as_arru8();
        formatter.write_str(&to_hex_string(&bytes))
    }
}

impl Bootloop {
    pub fn new() -> Bootloop {
        Default::default()
    }
    pub fn get_as_arru8(&self) -> [u8; BL_SIZE / 8] {
        slicebool_to_arru8(&self.bl[..])
    }
    pub fn set_from_string(&mut self, s: &str) -> bool {
        if let Some(tmp) = from_string::<BL_SIZE>(s) {
            self.bl = tmp;
            true
        } else {
            false
        }
    }
    pub fn load_from_loop(&mut self, inp: &Loop) -> Option<usize> {
        let mut state = SyncState::WaitPreSync;
        let mut max_bits = 7833;
        let mut cnt = 0;
        let mut idx = 0;
        while max_bits > 0 {
            let bit = inp.get(idx);
            match state {
                SyncState::WaitPreSync => {
                    if bit {
                        cnt = 3157;
                        state = SyncState::WaitPreSyncMinLen;
                    }
                }
                SyncState::WaitPreSyncMinLen => {
                    if bit {
                        cnt -= 1;
                        if cnt == 0 {
                            state = SyncState::WaitPreSyncEnd;
                        }
                    } else {
                        state = SyncState::WaitPreSync;
                    }
                }
                SyncState::WaitPreSyncEnd => {
                    if !bit {
                        cnt = 255;
                        state = SyncState::WaitSyncPattern;
                    }
                }
                SyncState::WaitSyncPattern => {
                    let exp = if cnt > 14 { false } else { (cnt & 1) == 0 };
                    if bit == exp {
                        cnt -= 1;
                        if cnt == 0 {
                            cnt = BL_SIZE;
                            state = SyncState::ReadBL;
                        }
                    } else {
                        state = SyncState::WaitPreSync;
                    }
                }
                SyncState::ReadBL => {
                    self.bl[BL_SIZE - cnt] = bit;
                    cnt -= 1;
                    if cnt == 0 {
                        return Some(idx);
                    }
                }
            }
            idx = (idx + 1) % BITS_PER_LOOP;
            max_bits -= 1;
        }
        None
    }
    pub fn save_to_loop(&self, off: usize) -> Loop {
        let mut lp = Loop::new();
        let mut idx =
            off + 1 + BITS_PER_LOOP - BL_PRE_SYNC_BITS - BL_SYNC_BITS - BL_PATTERN_BITS - BL_SIZE;
        for _ in 0..BL_PRE_SYNC_BITS {
            lp.set(idx, true);
            idx += 1;
        }
        for _ in 0..BL_SYNC_BITS {
            lp.set(idx, false);
            idx += 1;
        }
        for i in 0..BL_PATTERN_BITS {
            lp.set(idx, (i & 1) == 0);
            idx += 1;
        }
        for bit in self.bl {
            lp.set(idx, bit);
            idx += 1;
        }
        for _ in 0..BL_PAD_BITS {
            lp.set(idx, false);
            idx += 1;
        }
        lp
    }
    pub fn get_channel(&self, ch: Channel) -> ChannelBootloop {
        let mut it = self.bl.iter();
        if let Channel::ChannelB = ch {
            it.next();
        }
        let mut out = [false; CH_BL_SIZE];
        let it_out = out.iter_mut();
        for (o, v) in it_out.zip(it.step_by(2)) {
            *o = *v;
        }
        ChannelBootloop { bl: out }
    }
    pub fn set_from_channel(&mut self, ch: Channel, chbl: &ChannelBootloop) {
        let mut it_out = self.bl.iter_mut();
        if let Channel::ChannelB = ch {
            it_out.next();
        }
        for (o, v) in it_out.step_by(2).zip(chbl.bl.into_iter()) {
            *o = v;
        }
    }
}

#[derive(Clone, Copy)]
pub struct ChannelBootloop {
    bl: [bool; CH_BL_SIZE],
}

impl Default for ChannelBootloop {
    fn default() -> Self {
        ChannelBootloop {
            bl: [false; CH_BL_SIZE],
        }
    }
}

// b7bbb3fb7fff379ffbfff77fbd9bfbfff5fffbffffff97ffbf5fbbddefbfdfa5ff7fffffffffffff

const DEF_CHA_BL: &str = "dfdf7f5bffd7ebffcfffff9ff3faffbcf7ffffff";
const DEF_CHB_BL: &str = "755dff77dfff75dfffdfff7f7f5fb7f3ffffffff";

impl fmt::Display for ChannelBootloop {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.get_as_arru8();
        formatter.write_str(&to_hex_string(&bytes))
    }
}

impl ChannelBootloop {
    pub fn new() -> ChannelBootloop {
        Default::default()
    }
    pub fn ones(&self) -> u32 {
        self.bl.iter().map(|v| *v as u32).sum()
    }
    pub fn get_as_arru8(&self) -> [u8; CH_BL_SIZE / 8] {
        slicebool_to_arru8(&self.bl[..])
    }
    pub fn set_from_string(&mut self, s: &str) -> bool {
        if let Some(tmp) = from_string::<CH_BL_SIZE>(s) {
            self.bl = tmp;
            true
        } else {
            false
        }
    }
    pub fn set_default(&mut self, ch: Channel) {
        match ch {
            Channel::ChannelA => {
                self.set_from_string(DEF_CHA_BL);
            }
            Channel::ChannelB => {
                self.set_from_string(DEF_CHB_BL);
            }
        }
    }
    pub fn set_random(&mut self, ones: usize) {
        self.bl = [false; CH_BL_SIZE];
        let mut rnd = rand::rng();
        let mut cnt = ones;
        while cnt > 0 {
            let idx = rnd.random_range(0..CH_BL_SIZE);
            if !self.bl[idx] {
                self.bl[idx] = true;
                cnt -= 1;
            }
        }
    }
}

#[derive(Copy, Clone)]
pub struct Loop {
    bits: [u64; BITS_PER_LOOP / 64],
}

impl Default for Loop {
    fn default() -> Self {
        Loop {
            bits: [0; BITS_PER_LOOP / 64],
        }
    }
}
impl Loop {
    pub fn new() -> Loop {
        Default::default()
    }
    fn get_mask_off(idx: usize) -> (u64, usize) {
        let mask = 1_u64 << (idx % 64);
        let off = idx / 64;
        (mask, off)
    }
    pub fn get(&self, idx: usize) -> bool {
        let (mask, off) = Self::get_mask_off(idx & LOOP_IDX_MASK);
        mask & self.bits[off] != 0
    }
    pub fn set(&mut self, idx: usize, bit: bool) {
        let (mask, off) = Self::get_mask_off(idx & LOOP_IDX_MASK);
        if bit {
            self.bits[off] |= mask;
        } else {
            self.bits[off] &= !mask;
        }
    }
    pub fn decode(&mut self, inp: &[u8; ENC_LOOP_SIZE]) {
        let mut mask: u8 = 1;
        let mut off: usize = 0;
        for i in 0..BITS_PER_LOOP {
            self.set(i, inp[off] & mask != 0);
            mask <<= 1;
            if mask == 0 {
                mask = 1;
                off += 1;
            }
        }
    }
    pub fn encode(&self, out: &mut [u8; ENC_LOOP_SIZE]) {
        let mut mask = 0_u8;
        let mut off = 0_usize;
        for i in 0..BITS_PER_LOOP {
            if mask == 0 {
                mask = 1;
                out[off] = 0;
            }
            let bit = self.get(i);
            if bit {
                out[off] |= mask;
            }
            mask <<= 1;
            if mask == 0 {
                off += 1;
            }
        }
    }
}

#[derive(Copy, Clone)]
pub struct ChannelMemory {
    bl: ChannelBootloop,
    data_loops: [[Loop; LOOPS_PER_QUAD]; QUADS_PER_CH],
    offset: Option<Address>,
}

impl Default for ChannelMemory {
    fn default() -> Self {
        ChannelMemory {
            bl: ChannelBootloop::new(),
            data_loops: [[Loop::new(); LOOPS_PER_QUAD]; QUADS_PER_CH],
            offset: None,
        }
    }
}

impl ChannelMemory {
    pub fn new() -> ChannelMemory {
        Default::default()
    }
    pub fn load<R: Read>(&mut self, inp: &mut R, ch: Channel) -> io::Result<()> {
        let mut mem = ChannelMemory::new();

        let mut pezzo: [u8; ENC_LOOP_SIZE] = [0; ENC_LOOP_SIZE];

        inp.read_exact(&mut pezzo)?;
        let mut bl_loop = Loop::new();
        let mut bl = Bootloop::new();
        bl_loop.decode(&pezzo);
        let offset = bl.load_from_loop(&bl_loop);
        if let Some(off) = offset {
            mem.offset = Some(off as Address);
            mem.bl = bl.get_channel(ch);
        }
        for i in 0..QUADS_PER_CH {
            for j in 0..LOOPS_PER_QUAD {
                inp.read_exact(&mut pezzo)?;
                mem.data_loops[i][j].decode(&pezzo);
            }
        }
        *self = mem;
        Ok(())
    }
    pub fn save<W: Write>(&self, out: &mut W, bl: &Bootloop) -> io::Result<()> {
        if let Some(off) = self.offset {
            let bl_loop = bl.save_to_loop(off as usize);
            let mut pezzo: [u8; ENC_LOOP_SIZE] = [0; ENC_LOOP_SIZE];
            bl_loop.encode(&mut pezzo);
            out.write_all(&pezzo)?;
            for i in 0..QUADS_PER_CH {
                for j in 0..LOOPS_PER_QUAD {
                    self.data_loops[i][j].encode(&mut pezzo);
                    out.write_all(&pezzo)?;
                }
            }
            Ok(())
        } else {
            io::Result::Err(Error::other("Not synched"))
        }
    }
    pub fn get_bl(&self) -> &ChannelBootloop {
        &self.bl
    }
    #[allow(unused)]
    pub fn get_offset(&self) -> Option<Address> {
        self.offset
    }
    pub fn clear_data(&mut self) {
        self.data_loops = [[Loop::new(); LOOPS_PER_QUAD]; QUADS_PER_CH];
        self.offset = Some(DEF_OFFSET);
    }
    pub fn set_bl(&mut self, chbl: &ChannelBootloop) {
        self.bl = *chbl;
        self.clear_data();
    }
    pub fn pa_to_la(mut pa: Address) -> (Address, u8) {
        let b0 = pa & 1;
        pa >>= 1;
        let b1 = pa & 1;
        let n_steps = ((pa & 0x7fe) | b0) as u32;
        let pa = ((n_steps * LA_DIST as u32) % (LOG_ADDRS as u32)) as Address;
        (pa, if b1 != 0 { b0 as u8 + 1 } else { 0 })
    }
    pub fn la_to_pa(la: Address) -> Address {
        let pa0 = (la as u32 * LA_DIST_INV as u32) % (LOG_ADDRS as u32);
        let pa_b0 = pa0 & 1;
        ((pa0 - pa_b0) * 2 + pa_b0) as Address
    }
    pub fn pa_off(pa: Address, off: Address) -> Address {
        (pa + off) & PA_MASK
    }
    fn get_offset_pa(&self, pa: Address) -> Option<Address> {
        let off = self.offset?;
        Some(Self::pa_off(Self::pa_off(pa, off), SYNC_OFFSET))
    }
    pub fn get_masked_bl(&self, ec: bool) -> ChannelBootloop {
        let mut cnt = ChannelMemory::bits_per_ch(ec) / 2;
        let mut out = [false; CH_BL_SIZE];
        for (it_in, it_out) in zip(self.bl.bl.iter(), out.iter_mut()) {
            let mut b = *it_in;
            if cnt != 0 && b {
                cnt -= 1;
            } else {
                b = false;
            }
            *it_out = b;
        }
        ChannelBootloop { bl: out }
    }
    pub fn get_page(&self, la: Address, ec: bool) -> Option<Vec<bool>> {
        let pa = self.get_offset_pa(Self::la_to_pa(la))?;
        let masked_bl = self.get_masked_bl(ec);
        let mut out: Vec<bool> = Vec::new();
        let mut idx = 0;
        let sz = Self::bits_per_ch(ec);
        while out.len() < sz {
            let odd = idx % 2;
            let blr_idx = ((idx >> 1) & !1) + odd;
            if masked_bl.bl[blr_idx] {
                let eff_pa = if (idx & 2) == 0 {
                    pa
                } else {
                    Self::pa_off(pa, 2)
                };
                let bit =
                    self.data_loops[1 - odd][LOOPS_PER_QUAD - 1 - idx / 4].get(eff_pa as usize);
                out.push(bit);
            }
            idx += 1;
            if idx == BL_SIZE {
                return None;
            }
        }
        Some(out)
    }
    pub fn get_page_pa(&self, pa: Address, skip_bad: bool) -> Option<Vec<bool>> {
        let off_pa = self.get_offset_pa(pa)?;
        let mut out: Vec<bool> = Vec::new();
        let bl = &self.bl.bl;
        for (idx, bl_bit) in bl.iter().enumerate() {
            if !skip_bad || *bl_bit {
                let odd = idx % 2;
                out.push(
                    self.data_loops[1 - odd][LOOPS_PER_QUAD - 1 - idx / 2].get(off_pa as usize),
                );
            }
        }
        Some(out)
    }
    pub fn set_page(&mut self, la: Address, ec: bool, page: &[bool]) -> Result<(), ()> {
        let sz = Self::bits_per_ch(ec);
        if page.len() != sz {
            Err(())
        } else {
            let pa = self.get_offset_pa(Self::la_to_pa(la)).ok_or(())?;
            let masked_bl = self.get_masked_bl(ec);
            let mut idx = 0;
            let mut page_idx = 0;
            while page_idx < sz {
                let odd = idx % 2;
                let blr_idx = ((idx >> 1) & !1) + odd;
                if masked_bl.bl[blr_idx] {
                    let eff_pa = if (idx & 2) == 0 {
                        pa
                    } else {
                        Self::pa_off(pa, 2)
                    };
                    let bit = page[page_idx];
                    page_idx += 1;
                    self.data_loops[1 - odd][LOOPS_PER_QUAD - 1 - idx / 4]
                        .set(eff_pa as usize, bit);
                }
                idx += 1;
                if idx == BL_SIZE {
                    return Err(());
                }
            }
            Ok(())
        }
    }
    pub fn set_page_pa(&mut self, pa: Address, skip_bad: bool, page: &[bool]) -> Result<(), ()> {
        let min_len = if skip_bad {
            self.bl.ones() as usize
        } else {
            CH_BL_SIZE
        };
        if page.len() >= min_len {
            let off_pa = self.get_offset_pa(pa).ok_or(())?;
            let bl = &self.bl.bl;
            let mut it = page.iter();
            for (idx, bl_bit) in bl.iter().enumerate() {
                let bit = if !skip_bad || *bl_bit {
                    *it.next().unwrap()
                } else {
                    false
                };
                let odd = idx % 2;
                self.data_loops[1 - odd][LOOPS_PER_QUAD - 1 - idx / 2].set(off_pa as usize, bit);
            }
            Ok(())
        } else {
            Err(())
        }
    }
    pub fn bits_per_ch(ec: bool) -> usize {
        if ec { EC_BITS } else { NO_EC_BITS }
    }
    fn is_bl_valid(&self) -> bool {
        self.bl.ones() >= MIN_ONES as u32
    }
}

#[derive(Clone, Copy)]
pub enum FireResult {
    Ok,
    OkNoECC,
    LoadError,
    Correctable,
    Uncorrectable,
}

pub struct PayloadNCode {
    payload: Vec<bool>,
    code: u16,
}

#[derive(Clone)]
pub struct FireCode {
    state: FireResult,
    uncorr_code: Vec<bool>,
    corr_code: Vec<bool>,
    accum: u16,
}

/// Length of Fire code
const FIRE_CODE_BITS: usize = 14;
/// Mask of MSB of Fire code (bit 13)
const FIRE_MSB_MASK: u16 = 1_u16 << (FIRE_CODE_BITS - 1);
/// Mask of Fire code bits
const FIRE_MASK: u16 = (1_u16 << FIRE_CODE_BITS) - 1;
/// Polynomial of Fire code
const FIRE_POLY: u16 = 0b101000100101;
/// Mask of error trap (lower 9 bits)
const ERR_TRAP_MASK: u16 = (1_u16 << 9) - 1;
/// Bits in error-corrected payload
const PAYLOAD_BITS: usize = 256;
/// Bits chopped off (279,265) Fire code
const FIRE_CHOPPED_BITS: usize = 9;

impl FireCode {
    pub fn new() -> Self {
        FireCode {
            state: FireResult::LoadError,
            uncorr_code: Vec::new(),
            corr_code: Vec::new(),
            accum: 0,
        }
    }
    pub fn set_page(&mut self, ec: bool, page: &[bool]) -> FireResult {
        self.state = self.int_load(ec, page);
        self.state
    }
    pub fn get_page(&self) -> &[bool] {
        &self.uncorr_code
    }
    #[allow(unused)]
    pub fn get_state(&self) -> FireResult {
        self.state
    }
    pub fn get_payload_n_code(&self) -> PayloadNCode {
        let mut out = PayloadNCode {
            payload: Vec::new(),
            code: 0,
        };
        match self.state {
            FireResult::Ok | FireResult::Correctable | FireResult::Uncorrectable => {
                out.payload = self
                    .uncorr_code
                    .iter()
                    .take(PAYLOAD_BITS)
                    .copied()
                    .collect();
                out.code = self
                    .uncorr_code
                    .iter()
                    .skip(PAYLOAD_BITS)
                    .take(FIRE_CODE_BITS)
                    .fold(0, |acc, b| (acc << 1) | (*b) as u16);
            }
            FireResult::OkNoECC => {
                out.payload = self.uncorr_code.iter().take(NO_EC_BITS).copied().collect();
            }
            _ => {}
        }
        out
    }
    pub fn apply_correction(&mut self) -> FireResult {
        if let FireResult::Correctable = self.state {
            self.uncorr_code = self.corr_code.clone();
            self.state = FireResult::Ok;
        }
        self.state
    }
    pub fn set_payload(&mut self, ec: bool, payload: &[bool]) -> FireResult {
        let exp_len = if ec { PAYLOAD_BITS } else { NO_EC_BITS };
        if payload.len() != exp_len {
            self.state = FireResult::LoadError;
        } else if ec {
            let mut in_code = payload.to_vec();
            in_code.extend(
                self.uncorr_code
                    .iter()
                    .skip(PAYLOAD_BITS)
                    .take(FIRE_CODE_BITS),
            );
            self.state = self.int_load(ec, &in_code);
        } else {
            self.state = self.int_load(ec, payload);
        }
        self.state
    }
    pub fn set_fire(&mut self, code: u16) -> FireResult {
        match self.state {
            FireResult::Correctable | FireResult::Ok | FireResult::Uncorrectable => {
                let mut in_code: Vec<bool> = self
                    .uncorr_code
                    .iter()
                    .take(PAYLOAD_BITS)
                    .copied()
                    .collect();
                for idx in 0..FIRE_CODE_BITS {
                    let bit = (code & (FIRE_MSB_MASK >> idx)) != 0;
                    in_code.push(bit);
                }
                self.state = self.int_load(true, &in_code);
            }
            _ => {}
        }
        self.state
    }
    pub fn regen_code(&mut self) -> FireResult {
        match self.state {
            FireResult::Correctable | FireResult::Ok | FireResult::Uncorrectable => {
                let regen_code: u16 = self
                    .uncorr_code
                    .iter()
                    .take(PAYLOAD_BITS)
                    .fold(0, |acc, bit| Self::fire_code_enc(acc, *bit));
                self.set_fire(regen_code);
            }
            _ => {}
        }
        self.state
    }
    pub fn add_code_to_payload(payload: &mut Vec<bool>) {
        let code: u16 = payload
            .iter()
            .fold(0, |acc, bit| Self::fire_code_enc(acc, *bit));
        for idx in 0..FIRE_CODE_BITS {
            let bit = (code & (FIRE_MSB_MASK >> idx)) != 0;
            payload.push(bit);
        }
    }
    fn int_load(&mut self, ec: bool, in_code: &[bool]) -> FireResult {
        let exp_len = ChannelMemory::bits_per_ch(ec);
        if in_code.len() != exp_len {
            FireResult::LoadError
        } else if ec {
            self.uncorr_code = in_code.to_vec();
            self.accum = 0;
            self.corr_code = in_code.to_vec();
            for bit in in_code {
                self.fire_code_syn(*bit);
            }
            if self.accum == 0 {
                FireResult::Ok
            } else {
                self.correct()
            }
        } else {
            self.uncorr_code = in_code.to_vec();
            FireResult::OkNoECC
        }
    }
    fn fire_code_enc(acc: u16, bit: bool) -> u16 {
        let mut accum = acc;
        if bit ^ ((accum & FIRE_MSB_MASK) != 0) {
            accum = (accum << 1) ^ FIRE_POLY;
        } else {
            accum <<= 1;
        }
        accum & FIRE_MASK
    }
    fn fire_code_syn(&mut self, bit: bool) {
        self.accum = Self::fire_code_enc(self.accum, false);
        self.accum ^= bit as u16;
    }
    fn correct(&mut self) -> FireResult {
        enum CorrectState {
            WaitTrap,
            WaitToStart,
            Correcting,
            Done,
        }
        let mut state = CorrectState::WaitTrap;
        let mut cnt = 0;
        for i in 0..FIRE_CODE_BITS {
            if (self.accum & ERR_TRAP_MASK) == 0 {
                cnt = i + PAYLOAD_BITS + 1;
                state = CorrectState::WaitToStart;
                break;
            } else {
                self.fire_code_syn(false);
            }
        }
        for _ in 0..FIRE_CHOPPED_BITS {
            if let CorrectState::WaitTrap = state {
                if (self.accum & ERR_TRAP_MASK) == 0 {
                    state = CorrectState::Correcting;
                } else {
                    self.fire_code_syn(false);
                }
            }
            if let CorrectState::Correcting = state {
                self.accum = (self.accum << 1) & FIRE_MASK;
                if self.accum == 0 {
                    state = CorrectState::Done;
                }
            }
        }
        for i in 0..EC_BITS {
            if let CorrectState::WaitTrap = state {
                if i < PAYLOAD_BITS {
                    if (self.accum & ERR_TRAP_MASK) == 0 {
                        state = CorrectState::Correcting;
                    } else {
                        self.fire_code_syn(false);
                    }
                }
            }
            if let CorrectState::WaitToStart = state {
                cnt -= 1;
                if cnt == 0 {
                    state = CorrectState::Correcting;
                }
            }
            let mut err = false;
            if let CorrectState::Correcting = state {
                err = (self.accum & FIRE_MSB_MASK) != 0;
                self.accum = (self.accum << 1) & FIRE_MASK;
                if self.accum == 0 {
                    state = CorrectState::Done;
                }
            }
            self.corr_code[i] ^= err;
        }
        if let CorrectState::WaitTrap = state {
            FireResult::Uncorrectable
        } else {
            FireResult::Correctable
        }
    }
}

#[derive(Clone, Copy)]
enum CurrentAddr {
    None,
    LA(Address),
    PA(Address),
}

#[derive(Clone)]
pub struct Memory {
    chs: [ChannelMemory; CH_PER_MEM],
    fires: [FireCode; CH_PER_MEM],
    curr_addr: CurrentAddr,
    curr_ec: bool,
    curr_skip: bool,
    dirty: bool,
}

impl Default for Memory {
    fn default() -> Self {
        Memory {
            chs: [ChannelMemory::new(); CH_PER_MEM],
            fires: [FireCode::new(), FireCode::new()],
            curr_addr: CurrentAddr::None,
            curr_ec: false,
            curr_skip: false,
            dirty: false,
        }
    }
}

impl Memory {
    pub fn new() -> Self {
        Default::default()
    }
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    pub fn get_ch(&self, ch: Channel) -> &ChannelMemory {
        &self.chs[ch as usize]
    }
    pub fn clear(&mut self) {
        let mut chbl_a = ChannelBootloop::new();
        chbl_a.set_default(Channel::ChannelA);
        self.chs[Channel::ChannelA as usize] = ChannelMemory::new();
        self.chs[Channel::ChannelA as usize].set_bl(&chbl_a);
        self.fires[Channel::ChannelA as usize] = FireCode::new();
        let mut chbl_b = ChannelBootloop::new();
        chbl_b.set_default(Channel::ChannelB);
        self.chs[Channel::ChannelB as usize] = ChannelMemory::new();
        self.chs[Channel::ChannelB as usize].set_bl(&chbl_b);
        self.fires[Channel::ChannelB as usize] = FireCode::new();
        self.curr_addr = CurrentAddr::None;
        self.dirty = false;
    }
    pub fn data_clear(&mut self) {
        self.chs[Channel::ChannelA as usize].clear_data();
        self.fires[Channel::ChannelA as usize] = FireCode::new();
        self.chs[Channel::ChannelB as usize].clear_data();
        self.fires[Channel::ChannelB as usize] = FireCode::new();
        self.curr_addr = CurrentAddr::None;
        self.dirty = false;
    }
    pub fn load<R: Read>(&mut self, inp: &mut R) -> io::Result<()> {
        let mut mem = Self::new();
        mem.chs[Channel::ChannelA as usize].load(inp, Channel::ChannelA)?;
        mem.chs[Channel::ChannelB as usize].load(inp, Channel::ChannelB)?;
        // This also sets dirty = false
        *self = mem;
        Ok(())
    }
    pub fn data_load<R: Read>(&mut self, inp: &mut R) -> io::Result<()> {
        let mut image = [0_u8; IMG_SIZE];
        inp.read_exact(&mut image)?;
        for la in 0..LOG_ADDRS {
            let page_idx = (la as usize) * LOG_PAGE_SIZE;
            let sl = &image[page_idx..(page_idx + LOG_PAGE_SIZE)];
            let vb = sliceu8_to_vecbool(sl);
            let (mut a, mut b) = Self::deinterleave(&vb);
            FireCode::add_code_to_payload(&mut a);
            let _ = self.chs[Channel::ChannelA as usize].set_page(la, true, &a);
            FireCode::add_code_to_payload(&mut b);
            let _ = self.chs[Channel::ChannelB as usize].set_page(la, true, &b);
        }
        self.curr_addr = CurrentAddr::None;
        self.dirty = true;
        Ok(())
    }
    pub fn save<W: Write>(&mut self, out: &mut W) -> io::Result<()> {
        let ch_a = self.chs[Channel::ChannelA as usize].get_bl();
        let ch_b = self.chs[Channel::ChannelB as usize].get_bl();
        let mut bl = Bootloop::new();
        bl.set_from_channel(Channel::ChannelA, ch_a);
        bl.set_from_channel(Channel::ChannelB, ch_b);
        self.chs[Channel::ChannelA as usize].save(out, &bl)?;
        self.chs[Channel::ChannelB as usize].save(out, &bl)?;
        self.dirty = false;
        Ok(())
    }
    pub fn data_save<W: Write>(&mut self, out: &mut W) -> io::Result<(usize, usize, usize)> {
        let mut image = [0_u8; IMG_SIZE];
        let mut cnt_ok = 0_usize;
        let mut cnt_corrected = 0_usize;
        let mut cnt_uncorrectable = 0_usize;
        for la in 0..LOG_ADDRS {
            let (res_a, res_b) = self.get_la_page(la, true);
            match res_a {
                FireResult::LoadError => {
                    return io::Result::Err(Error::other("LoadError"));
                }
                FireResult::Ok => {
                    cnt_ok += 1;
                }
                FireResult::Correctable => {
                    self.fires[Channel::ChannelA as usize].apply_correction();
                    cnt_corrected += 1;
                }
                FireResult::Uncorrectable => {
                    cnt_uncorrectable += 1;
                }
                _ => {}
            }
            match res_b {
                FireResult::LoadError => {
                    return io::Result::Err(Error::other("LoadError"));
                }
                FireResult::Ok => {
                    cnt_ok += 1;
                }
                FireResult::Correctable => {
                    self.fires[Channel::ChannelB as usize].apply_correction();
                    cnt_corrected += 1;
                }
                FireResult::Uncorrectable => {
                    cnt_uncorrectable += 1;
                }
                _ => {}
            }
            let pay_a = &self.fires[Channel::ChannelA as usize]
                .get_payload_n_code()
                .payload;
            let pay_b = &self.fires[Channel::ChannelB as usize]
                .get_payload_n_code()
                .payload;
            let pagev = Self::interleave(pay_a, pay_b);
            let page = slicebool_to_vecu8(&pagev);
            let out = &mut image[la as usize * LOG_PAGE_SIZE..(la + 1) as usize * LOG_PAGE_SIZE];
            out.copy_from_slice(&page);
        }
        self.curr_addr = CurrentAddr::None;
        out.write_all(&image)?;
        Ok((cnt_ok, cnt_corrected, cnt_uncorrectable))
    }
    pub fn data_fix(&mut self, actually_fix: bool) -> Option<(usize, usize, usize)> {
        let mut cnt_ok = 0_usize;
        let mut cnt_corrected = 0_usize;
        let mut cnt_uncorrectable = 0_usize;
        for la in 0..LOG_ADDRS {
            let (res_a, res_b) = self.get_la_page(la, true);
            match res_a {
                FireResult::LoadError => {
                    return None;
                }
                FireResult::Ok => {
                    cnt_ok += 1;
                }
                FireResult::Correctable => {
                    if actually_fix {
                        self.fires[Channel::ChannelA as usize].apply_correction();
                        let page = self.fires[Channel::ChannelA as usize].get_page();
                        let res = self.chs[Channel::ChannelA as usize].set_page(la, true, page);
                        if res.is_err() {
                            return None;
                        }
                        self.dirty = true;
                    }
                    cnt_corrected += 1;
                }
                FireResult::Uncorrectable => {
                    cnt_uncorrectable += 1;
                }
                _ => {}
            }
            match res_b {
                FireResult::LoadError => {
                    return None;
                }
                FireResult::Ok => {
                    cnt_ok += 1;
                }
                FireResult::Correctable => {
                    if actually_fix {
                        self.fires[Channel::ChannelB as usize].apply_correction();
                        let page = self.fires[Channel::ChannelB as usize].get_page();
                        let res = self.chs[Channel::ChannelB as usize].set_page(la, true, page);
                        if res.is_err() {
                            return None;
                        }
                        self.dirty = true;
                    }
                    cnt_corrected += 1;
                }
                FireResult::Uncorrectable => {
                    cnt_uncorrectable += 1;
                }
                _ => {}
            }
        }
        self.curr_addr = CurrentAddr::None;
        Some((cnt_ok, cnt_corrected, cnt_uncorrectable))
    }
    pub fn get_la_page(&mut self, la: Address, ec: bool) -> (FireResult, FireResult) {
        let mut res_a = FireResult::LoadError;
        let mut res_b = FireResult::LoadError;
        let page_a = self.chs[Channel::ChannelA as usize].get_page(la, ec);
        if let Some(vec_a) = page_a {
            res_a = self.fires[Channel::ChannelA as usize].set_page(ec, &vec_a);
        }
        let page_b = self.chs[Channel::ChannelB as usize].get_page(la, ec);
        if let Some(vec_b) = page_b {
            res_b = self.fires[Channel::ChannelB as usize].set_page(ec, &vec_b);
        }
        self.curr_addr = CurrentAddr::LA(la);
        self.curr_ec = ec;
        (res_a, res_b)
    }
    fn interleave(a: &[bool], b: &[bool]) -> Vec<bool> {
        let mut out: Vec<bool> = Vec::new();
        for (a, b) in zip(a.iter(), b.iter()) {
            out.push(*a);
            out.push(*b);
        }
        out
    }
    pub fn get_payload(&self) -> Vec<u8> {
        if let CurrentAddr::LA(_) = self.curr_addr {
            let pay_a = self.fires[Channel::ChannelA as usize].get_payload_n_code();
            let pay_b = self.fires[Channel::ChannelB as usize].get_payload_n_code();
            let interleaved = Self::interleave(&pay_a.payload, &pay_b.payload);
            slicebool_to_vecu8(&interleaved)
        } else {
            Vec::<u8>::new()
        }
    }
    fn deinterleave(inter: &[bool]) -> (Vec<bool>, Vec<bool>) {
        let mut a: Vec<bool> = Vec::new();
        let mut b: Vec<bool> = Vec::new();
        let mut it = inter.iter();
        while let Some(xa) = it.next() {
            a.push(*xa);
            if let Some(xb) = it.next() {
                b.push(*xb);
            } else {
                break;
            }
        }
        (a, b)
    }
    pub fn set_payload(&mut self, pay: &[u8]) -> Result<(), ()> {
        let pay_bool = sliceu8_to_vecbool(pay);
        let (pay_a, pay_b) = Self::deinterleave(&pay_bool);
        if let FireResult::LoadError =
            self.fires[Channel::ChannelA as usize].set_payload(self.curr_ec, &pay_a)
        {
            Err(())
        } else if let FireResult::LoadError =
            self.fires[Channel::ChannelB as usize].set_payload(self.curr_ec, &pay_b)
        {
            Err(())
        } else {
            self.update_page(Channel::ChannelA)?;
            self.update_page(Channel::ChannelB)?;
            Ok(())
        }
    }
    pub fn payload_to_string(&self) -> String {
        let pay = self.get_payload();
        to_hex_string(&pay)
    }
    pub fn payload_to_ascii(&self) -> String {
        let pay = self.get_payload();
        let mut s = String::new();
        for b in pay {
            let co = char::from_u32(b as u32);
            let out_c = if let Some(c) = co {
                if c.is_ascii() && !c.is_ascii_control() {
                    c
                } else {
                    '•'
                }
            } else {
                ' '
            };
            s.push(out_c);
        }
        s
    }
    pub fn payload_from_string(&mut self, s: &str) -> Result<(), ()> {
        let exp_len = if self.curr_ec {
            PAYLOAD_BITS
        } else {
            NO_EC_BITS
        };
        if s.len() == exp_len / 2 {
            if let Some(out) = string_to_vecu8(s) {
                return self.set_payload(&out);
            }
        }
        Err(())
    }
    pub fn code_to_string(&self, ch: Channel) -> String {
        if let CurrentAddr::LA(_) = self.curr_addr {
            if self.curr_ec {
                let pay_n_code = self.fires[ch as usize].get_payload_n_code();
                return format!("{:04x}", pay_n_code.code);
            }
        }
        String::new()
    }
    pub fn code_from_string(&mut self, ch: Channel, s: &str) -> Result<(), ()> {
        let co = u16::from_str_radix(s, 16);
        if let Ok(c) = co {
            if let FireResult::LoadError = self.fires[ch as usize].set_fire(c) {
                Err(())
            } else {
                self.update_page(ch)
            }
        } else {
            Err(())
        }
    }
    pub fn apply_correction(&mut self, ch: Channel) -> Result<(), ()> {
        if self.curr_ec {
            if let FireResult::Ok = self.fires[ch as usize].apply_correction() {
                return self.update_page(ch);
            }
        }
        Err(())
    }
    pub fn regen_code(&mut self) -> Result<(), ()> {
        if self.curr_ec {
            if let FireResult::Ok = self.fires[Channel::ChannelA as usize].regen_code() {
                self.update_page(Channel::ChannelA)?;
            }
            if let FireResult::Ok = self.fires[Channel::ChannelB as usize].regen_code() {
                self.update_page(Channel::ChannelB)?;
            }
        }
        Ok(())
    }
    fn update_page(&mut self, ch: Channel) -> Result<(), ()> {
        if let CurrentAddr::LA(la) = self.curr_addr {
            let page = self.fires[ch as usize].get_page();
            self.chs[ch as usize].set_page(la, self.curr_ec, page)?;
            self.dirty = true;
            Ok(())
        } else {
            Err(())
        }
    }
    pub fn set_ch_bl(&mut self, cha: Option<&ChannelBootloop>, chb: Option<&ChannelBootloop>) {
        if let Some(chbl) = cha {
            self.chs[Channel::ChannelA as usize].set_bl(chbl);
        } else {
            self.chs[Channel::ChannelA as usize].clear_data();
        }
        if let Some(chbl) = chb {
            self.chs[Channel::ChannelB as usize].set_bl(chbl);
        } else {
            self.chs[Channel::ChannelB as usize].clear_data();
        }
        self.dirty = false;
    }
    pub fn get_page_pa(&mut self, ch: Channel, pa: Address, skip_bad: bool) -> Vec<u8> {
        let page_o = self.chs[ch as usize].get_page_pa(pa, skip_bad);
        if let Some(page) = page_o {
            self.curr_addr = CurrentAddr::PA(pa);
            self.curr_skip = skip_bad;
            slicebool_to_vecu8(&page)
        } else {
            Vec::<u8>::new()
        }
    }
    pub fn page_pa_to_string(&mut self, ch: Channel, pa: Address, skip_bad: bool) -> String {
        let page = self.get_page_pa(ch, pa, skip_bad);
        to_hex_string(&page)
    }
    pub fn page_pa_from_string(&mut self, ch: Channel, s: &str) -> Result<(), ()> {
        if let CurrentAddr::PA(pa) = self.curr_addr {
            if let Some(vec) = string_to_vecu8(s) {
                let page = sliceu8_to_vecbool(&vec);
                self.chs[ch as usize].set_page_pa(pa, self.curr_skip, &page)?;
                self.dirty = true;
                return Ok(());
            }
        }
        Err(())
    }
    pub fn is_bl_valid(&self) -> bool {
        self.chs[Channel::ChannelA as usize].is_bl_valid()
            && self.chs[Channel::ChannelB as usize].is_bl_valid()
    }
}
