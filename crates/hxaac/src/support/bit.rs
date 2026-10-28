// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::cmp::min;

use super::bits::*;
use super::io;

fn end_of_bitstream_error<T>() -> io::Result<T> {
    Err(io::Error::other("unexpected end of bitstream"))
}

pub mod vlc {
    //! The `vlc` module provides support for decoding variable-length codes (VLC).

    use alloc::collections::{BTreeMap, VecDeque};
    use alloc::vec::Vec;
    use super::super::io;
    use core::num::NonZero;

    fn codebook_error<T>(desc: &'static str) -> io::Result<T> {
        Err(io::Error::other(desc))
    }

    /// `BitOrder` describes the relationship between the order of bits in the provided codewords
    /// and the order in which bits are read.
    #[derive(Copy, Clone)]
    pub enum BitOrder {
        /// The provided codewords have bits in the same order as the order in which they're being
        /// read.
        Verbatim,
        /// The provided codeword have bits in the reverse order as the order in which they're
        /// being read.
        Reverse,
    }

    /// The `CodebookEntry` trait describes an entry in a codebook.
    ///
    /// This trait should be implemented by a zero-sized marker struct. When instantiating a
    /// `Codebook`, the marker struct implementing `CodebookEntry` is supplied as the type argument.
    pub trait CodebookEntry: Copy + Clone + Default {
        /// The type of an index into the codebook table.
        type IndexType: Copy + Default + TryFrom<u32> + Into<u32>;
        /// The type of a value in the codebook.
        type ValueType: Copy;
    }

    macro_rules! decl_entry {
        (
            #[doc = $expr:expr]
            $name:ident, $value_type:ty, $index_type:ty
        ) => {
            #[doc = $expr]
            #[derive(Copy, Clone, Default)]
            pub struct $name;

            impl CodebookEntry for $name {
                type IndexType = $index_type;
                type ValueType = $value_type;
            }
        };
    }

    decl_entry!(
        /// `Entry8x8` is a codebook entry for 8-bit values with codes up-to 8-bits.
        Entry8x8,
        u8,
        u8
    );

    decl_entry!(
        /// `Entry8x16` is a codebook entry for 8-bit values with codes up-to 16-bits.
        Entry8x16,
        u8,
        u16
    );

    decl_entry!(
        /// `Entry8x32` is a codebook entry for 8-bit values with codes up-to 32-bits.
        Entry8x32,
        u8,
        u32
    );

    decl_entry!(
        /// `Entry16x8` is a codebook entry for 16-bit values with codes up-to 8-bits.
        Entry16x8,
        u16,
        u8
    );

    decl_entry!(
        /// `Entry16x16` is a codebook entry for 16-bit values with codes up-to 16-bits.
        Entry16x16,
        u16,
        u16
    );

    decl_entry!(
        /// `Entry16x32` is a codebook entry for 16-bit values with codes up-to 32-bits.
        Entry16x32,
        u16,
        u32
    );

    decl_entry!(
        /// `Entry32x8` is a codebook entry for 32-bit values with codes up-to 8-bits.
        Entry32x8,
        u32,
        u8
    );

    decl_entry!(
        /// `Entry32x16` is a codebook entry for 32-bit values with codes up-to 16-bits.
        Entry32x16,
        u32,
        u16
    );

    decl_entry!(
        /// `Entry32x32` is a codebook entry for 32-bit values with codes up-to 32-bits.
        Entry32x32,
        u32,
        u32
    );

    /// The concerete type for a codebook entry. May be either a Jump or Value entry.
    #[derive(Copy, Clone)]
    pub(super) enum Entry<E: CodebookEntry> {
        /// A jump entry indicates to the decoder that the next codebook entry should be looked-up
        /// at the offset specified by the provided index plus the next set of codeword bits.
        Jump { index: E::IndexType },
        /// A value entry indicates to the decoder that a codeword with the provided length in bits
        /// has been decoded, and it yielded the provided value.
        Value { value: E::ValueType, code_len: core::num::NonZero<u8> },
    }

    impl<E: CodebookEntry> Default for Entry<E> {
        fn default() -> Self {
            Self::Jump { index: Default::default() }
        }
    }

    impl<E: CodebookEntry> Entry<E> {
        /// Try to create a new jump entry. Returns `None` if the index is out-of-bounds for the
        /// index type of the entry.
        pub fn new_jump(index: u32) -> Option<Self> {
            index.try_into().ok().map(|index| Self::Jump { index })
        }

        /// Create a new value entry.
        pub fn new_value(value: E::ValueType, code_len: core::num::NonZero<u8>) -> Self {
            Self::Value { value, code_len }
        }
    }

    /// `Codebook` is a variable-length code decoding table that may be used to efficiently read
    /// symbols from a source of bits.
    #[derive(Clone, Default)]
    pub struct Codebook<E: CodebookEntry> {
        /// The codebook entry table.
        pub(super) table: Vec<Entry<E>>,
        /// The maximum codeword length in bits in this codebook.
        pub(super) max_code_len: u32,
        /// The number of bits to read and decode at each iteration.
        pub(super) bits_per_block: u32,
    }

    impl<E: CodebookEntry> Codebook<E> {
        /// Returns `true` if the `Codebook` is empty.
        pub fn is_empty(&self) -> bool {
            self.table.is_empty()
        }
    }

    /// A codebook builder value entry.
    struct CodebookValue<E: CodebookEntry> {
        /// The remaining prefix bits of the codeword for this value.
        prefix: u16,
        /// The remaining number of prefix bits of the codeword for this value.
        width: u8,
        /// The total codeword length in number of bits for this value.
        code_len: NonZero<u8>,
        /// The value.
        value: E::ValueType,
    }

    impl<E: CodebookEntry> CodebookValue<E> {
        fn new(prefix: u16, width: u8, code_len: NonZero<u8>, value: E::ValueType) -> Self {
            Self { prefix, width, code_len, value }
        }
    }

    /// A codebook builder block. Represents a group of codebook table entries whose codewords share
    /// the same set of prefix bits upto this block.
    struct CodebookBlock<E: CodebookEntry> {
        /// The number of prefix bits the block consumes.
        width: u8,
        /// Map of unique child block prefixes to indicies in the codebook table.
        nodes: BTreeMap<u16, usize>,
        /// Codebook value entries whose codewords terminate in this block.
        values: Vec<CodebookValue<E>>,
    }

    impl<E: CodebookEntry> CodebookBlock<E> {
        fn new(width: u8) -> Self {
            Self { width, nodes: Default::default(), values: Default::default() }
        }
    }

    /// `CodebookBuilder` generates a `Codebook` using a provided codebook specification and
    /// description.
    pub struct CodebookBuilder {
        bits_per_block: u8,
        bit_order: BitOrder,
    }

    impl CodebookBuilder {
        /// Instantiates a new `CodebookBuilder`.
        ///
        /// The `bit_order` parameter specifies if the codeword bits should be reversed when
        /// constructing the codebook. If the `BitReader` or `BitStream` reading the constructed
        /// codebook reads bits in an order different from the order of the provided codewords,
        /// then this option can be used to make them compatible.
        pub fn new(bit_order: BitOrder) -> Self {
            CodebookBuilder { bits_per_block: 4, bit_order }
        }

        /// Specify the number of bits that should be consumed from the source at a time. This value
        /// must be within the range 1 <= `bits_per_read` <= 16. Values outside of this range will
        /// cause this function to panic. If not provided, a value will be automatically chosen.
        pub fn bits_per_read(&mut self, bits_per_read: u8) -> &mut Self {
            assert!(bits_per_read <= 16);
            assert!(bits_per_read > 0);
            self.bits_per_block = bits_per_read;
            self
        }

        fn generate_lut<E: CodebookEntry>(
            bit_order: BitOrder,
            blocks: &[CodebookBlock<E>],
        ) -> io::Result<Vec<Entry<E>>> {
            // The codebook table.
            let mut table = Vec::new();

            let mut queue = VecDeque::new();

            // The computed end of the table given the blocks in the queue.
            let mut table_end = 0u32;

            // If the table is not empty, prepare for the recursion.
            if let Some(block) = blocks.first() {
                // Start traversal at the first block.
                queue.push_front(0);
                // New blocks will begin after the end of the first block.
                table_end += 1 << block.width;
            }

            // Traverse the tree in breadth-first order.
            while let Some(block_id) = queue.pop_front() {
                // Count of the total number of entries added to the table by this block.
                let mut entry_count = 0;

                // Get the block at the front of the queue.
                let block = &blocks[block_id];
                let block_len = 1 << block.width;

                // The starting index of the current block.
                let table_base = table.len();

                // Resize the table to accomodate all entries within the block.
                table.resize(table_base + block_len, Default::default());

                // Push child blocks onto the queue and record the jump entries in the table. Jumps
                // will be in order of increasing prefix because of the implicit sorting provided
                // by BTreeMap, thus traversing a level of the tree left-to-right.
                for (&child_block_prefix, &child_block_id) in block.nodes.iter() {
                    queue.push_back(child_block_id);

                    // The width of the child block in bits.
                    let child_block_width = blocks[child_block_id].width;

                    // Try to create the jump entry. Return an error if the upper-bound of the
                    // jump's index type is exceeded.
                    let jump_entry = match Entry::<E>::new_jump(table_end) {
                        Some(e) => e,
                        _ => return codebook_error("core (io): codebook overflow"),
                    };

                    // Determine the offset into the table depending on the bit-order.
                    let offset = match bit_order {
                        BitOrder::Verbatim => child_block_prefix,
                        BitOrder::Reverse => {
                            child_block_prefix.reverse_bits().rotate_left(u32::from(block.width))
                        }
                    } as usize;

                    table[table_base + offset] = jump_entry;

                    // Add the length of the child block to the end of the table.
                    table_end += 1 << child_block_width;

                    // Update the entry count.
                    entry_count += 1;
                }

                // Add value entries into the table. If a value has a prefix width less than the
                // block width, then do-not-care bits must added to the end of the prefix to pad it
                // to the block width.
                for value in block.values.iter() {
                    // The number of do-not-care bits to add to the value's prefix.
                    let num_dnc_bits = block.width - value.width;

                    // Extend the value's prefix to the block's width.
                    let base_prefix = (value.prefix << num_dnc_bits) as usize;

                    // Using the base prefix, synthesize all prefixes for this value.
                    let count = 1 << num_dnc_bits;

                    // The value entry that will be duplicated.
                    let value_entry = Entry::<E>::new_value(value.value, value.code_len);

                    match bit_order {
                        BitOrder::Verbatim => {
                            // For verbatim bit order, the do-not-care bits are in the LSb
                            // position.
                            let start = table_base + base_prefix;
                            let end = start + count;

                            for entry in table[start..end].iter_mut() {
                                *entry = value_entry;
                            }
                        }
                        BitOrder::Reverse => {
                            // For reverse bit order, the do-not-care bits are in the MSb position.
                            let start = base_prefix;
                            let end = start + count;

                            for prefix in start..end {
                                let offset =
                                    prefix.reverse_bits().rotate_left(u32::from(block.width));

                                table[table_base + offset] = value_entry;
                            }
                        }
                    }

                    // Update the entry count.
                    entry_count += count;
                }

                // The number of entries added to the table should equal the block length. It is a
                // fatal error if this is not true.
                if entry_count != block_len {
                    return codebook_error("core (io): codebook is incomplete");
                }
            }

            Ok(table)
        }

        /// Construct a `Codebook` using the given codewords, their respective lengths in number of
        /// bits, and associated values.
        ///
        /// This function may fail if the provided codewords do not form a complete VLC tree, or if
        /// the `CodebookEntry` is undersized.
        ///
        /// # Panics
        ///
        /// Panics if the number of codewords, code lengths, and values differ.
        pub fn make<E: CodebookEntry>(
            &mut self,
            codes: &[u32],
            code_lens: &[u8],
            values: &[E::ValueType],
        ) -> io::Result<Codebook<E>> {
            assert!(codes.len() == code_lens.len());
            assert!(codes.len() == values.len());

            let mut blocks = Vec::<CodebookBlock<E>>::new();

            let mut max_code_len = 0;

            // Only attempt to generate something if there are code words.
            if !codes.is_empty() {
                let prefix_mask = !(!0 << self.bits_per_block);

                // Push a root block.
                blocks.push(CodebookBlock::new(self.bits_per_block));

                // Populate the tree
                for ((&code, &code_len), &value) in codes.iter().zip(code_lens).zip(values) {
                    let mut parent_block_id = 0;

                    // A zero length code if not allowed.
                    let code_len = match NonZero::<u8>::new(code_len) {
                        Some(len) => len,
                        _ => return codebook_error("core (io): zero length codeword"),
                    };

                    let mut len = code_len.get();

                    while len > self.bits_per_block {
                        len -= self.bits_per_block;

                        let prefix = ((code >> len) & prefix_mask) as u16;

                        // Recurse down the tree.
                        if let Some(&block_id) = blocks[parent_block_id].nodes.get(&prefix) {
                            parent_block_id = block_id;
                        }
                        else {
                            // Add a child block to the parent block.
                            let block_id = blocks.len();

                            let block = &mut blocks[parent_block_id];

                            block.nodes.insert(prefix, block_id);

                            // Append the new block.
                            blocks.push(CodebookBlock::new(self.bits_per_block));

                            parent_block_id = block_id;
                        }
                    }

                    // The final chunk of code bits always has <= bits_per_block bits. Obtain
                    // the final prefix.
                    let prefix = code & (prefix_mask >> (self.bits_per_block - len));

                    let block = &mut blocks[parent_block_id];

                    // Push the value.
                    block.values.push(CodebookValue::new(prefix as u16, len, code_len, value));

                    // Update maximum observed codeword length.
                    max_code_len = max_code_len.max(code_len.get());
                }
            }

            // Generate the codebook lookup table.
            let table = CodebookBuilder::generate_lut(self.bit_order, &blocks)?;

            Ok(Codebook {
                table,
                max_code_len: u32::from(max_code_len),
                bits_per_block: u32::from(self.bits_per_block),
            })
        }
    }
}

mod private {
    use super::super::io;

    pub trait FetchBitsLtr {
        /// Discard any remaining bits in the source and fetch 1 or more new bits.
        fn fetch_bits(&mut self) -> io::Result<()>;

        /// Fetch 0 or more new bits, and append them after the remaining bits.
        fn fetch_bits_partial(&mut self) -> io::Result<()>;

        /// Get all the bits in the source.
        fn get_bits(&self) -> u64;

        /// Get the number of bits left in the source.
        fn num_bits_left(&self) -> u32;

        /// Consume `num` bits from the source.
        fn consume_bits(&mut self, num: u32);
    }

    pub trait FetchBitsRtl {
        /// Discard any remaining bits in the source and fetch 1 or more new bits.
        fn fetch_bits(&mut self) -> io::Result<()>;

        /// Fetch 0 or more new bits, and append them after the remaining bits.
        fn fetch_bits_partial(&mut self) -> io::Result<()>;

        /// Get all the bits in the source.
        fn get_bits(&self) -> u64;

        /// Get the number of bits left in the source.
        fn num_bits_left(&self) -> u32;

        /// Consume `num` bits from the source.
        fn consume_bits(&mut self, num: u32);
    }
}

/// A `FiniteBitStream` is a bit stream that has a known length in bits.
pub trait FiniteBitStream {
    /// Gets the number of bits left unread.
    fn bits_left(&self) -> u64;
}

/// `ReadBitsLtr` reads bits from most-significant to least-significant.
pub trait ReadBitsLtr: private::FetchBitsLtr {
    /// Discards any saved bits and resets the `BitStream` to prepare it for a byte-aligned read.
    #[inline(always)]
    fn realign(&mut self) {
        let skip = self.num_bits_left() & 0x7;
        self.consume_bits(skip);
    }

    /// Ignores the specified number of bits from the stream or returns an error.
    #[inline(always)]
    fn ignore_bits(&mut self, mut num_bits: u32) -> io::Result<()> {
        if num_bits <= self.num_bits_left() {
            self.consume_bits(num_bits);
        }
        else {
            // Consume whole bit caches directly.
            while num_bits > self.num_bits_left() {
                num_bits -= self.num_bits_left();
                self.fetch_bits()?;
            }

            if num_bits > 0 {
                // Shift out in two parts to prevent panicing when num_bits == 64.
                self.consume_bits(num_bits - 1);
                self.consume_bits(1);
            }
        }

        Ok(())
    }

    /// Ignores one bit from the stream or returns an error.
    #[inline(always)]
    fn ignore_bit(&mut self) -> io::Result<()> {
        self.ignore_bits(1)
    }

    /// Read a single bit as a boolean value or returns an error.
    #[inline(always)]
    fn read_bool(&mut self) -> io::Result<bool> {
        if self.num_bits_left() < 1 {
            self.fetch_bits()?;
        }

        let bit = self.get_bits() & (1 << 63) != 0;

        self.consume_bits(1);
        Ok(bit)
    }

    /// Reads and returns a single bit or returns an error.
    #[inline(always)]
    fn read_bit(&mut self) -> io::Result<u32> {
        if self.num_bits_left() < 1 {
            self.fetch_bits()?;
        }

        let bit = self.get_bits() >> 63;

        self.consume_bits(1);

        Ok(bit as u32)
    }

    /// Reads and returns up to 32-bits or returns an error.
    #[inline(always)]
    fn read_bits_leq32(&mut self, mut bit_width: u32) -> io::Result<u32> {
        debug_assert!(bit_width <= u32::BITS);

        // Shift in two 32-bit operations instead of a single 64-bit operation to avoid panicing
        // when bit_width == 0 (and thus shifting right 64-bits). This is preferred to branching
        // the bit_width == 0 case, since reading up-to 32-bits at a time is a hot code-path.
        let mut bits = (self.get_bits() >> u32::BITS) >> (u32::BITS - bit_width);

        while bit_width > self.num_bits_left() {
            bit_width -= self.num_bits_left();

            self.fetch_bits()?;

            // Unlike the first shift, bit_width is always > 0 here so this operation will never
            // shift by > 63 bits.
            bits |= self.get_bits() >> (u64::BITS - bit_width);
        }

        self.consume_bits(bit_width);

        Ok(bits as u32)
    }

    /// Reads up to 32-bits and interprets them as a signed two's complement integer or returns an
    /// error.
    #[inline(always)]
    fn read_bits_leq32_signed(&mut self, bit_width: u32) -> io::Result<i32> {
        let value = self.read_bits_leq32(bit_width)?;
        Ok(sign_extend_leq32_to_i32(value, bit_width))
    }

    /// Reads and returns up to 64-bits or returns an error.
    #[inline(always)]
    fn read_bits_leq64(&mut self, mut bit_width: u32) -> io::Result<u64> {
        debug_assert!(bit_width <= u64::BITS);

        // Hard-code the bit_width == 0 case as it's not possible to handle both the bit_width == 0
        // and bit_width == 64 cases branchlessly. This should be optimized out when bit_width is
        // known at compile time. Since it's generally rare to need to read up-to 64-bits at a time
        // (as oppopsed to 32-bits), this is an acceptable solution.
        if bit_width == 0 {
            Ok(0)
        }
        else {
            // Since bit_width is always > 0, this shift operation is always < 64, and will
            // therefore never panic.
            let mut bits = self.get_bits() >> (u64::BITS - bit_width);

            while bit_width > self.num_bits_left() {
                bit_width -= self.num_bits_left();

                self.fetch_bits()?;

                bits |= self.get_bits() >> (u64::BITS - bit_width);
            }

            // Shift in two parts to prevent panicing when bit_width == 64.
            self.consume_bits(bit_width - 1);
            self.consume_bits(1);

            Ok(bits)
        }
    }

    /// Reads up to 64-bits and interprets them as a signed two's complement integer or returns an
    /// error.
    #[inline(always)]
    fn read_bits_leq64_signed(&mut self, bit_width: u32) -> io::Result<i64> {
        let value = self.read_bits_leq64(bit_width)?;
        Ok(sign_extend_leq64_to_i64(value, bit_width))
    }

    /// Reads and returns a unary zeros encoded integer or an error.
    #[inline(always)]
    fn read_unary_zeros(&mut self) -> io::Result<u32> {
        let mut num = 0;

        loop {
            // Get the number of leading zeros.
            let num_zeros = self.get_bits().leading_zeros();

            if num_zeros >= self.num_bits_left() {
                // If the number of zeros exceeds the number of bits left then all the remaining
                // bits were 0.
                num += self.num_bits_left();
                self.fetch_bits()?;
            }
            else {
                // Otherwise, a 1 bit was encountered after `n_zeros` 0 bits.
                num += num_zeros;

                // Since bits are shifted off the cache after they're consumed, for there to be a
                // 1 bit there must be atleast one extra available bit in the cache that can be
                // consumed after the 0 bits.
                self.consume_bits(num_zeros);
                self.consume_bits(1);

                // Done decoding.
                break;
            }
        }

        Ok(num)
    }

    /// Reads and returns a unary zeros encoded integer that is capped to a maximum value.
    #[inline(always)]
    fn read_unary_zeros_capped(&mut self, mut limit: u32) -> io::Result<u32> {
        let mut num = 0;

        loop {
            // Get the number of leading zeros, capped to the limit.
            let num_bits_left = self.num_bits_left();
            let num_zeros = min(self.get_bits().leading_zeros(), num_bits_left);

            if num_zeros >= limit {
                // There are more ones than the limit. A terminator cannot be encountered.
                num += limit;
                self.consume_bits(limit);
                break;
            }
            else {
                // There are less ones than the limit. A terminator was encountered OR more bits
                // are needed.
                limit -= num_zeros;
                num += num_zeros;

                if num_zeros < num_bits_left {
                    // There are less ones than the number of bits left in the reader. Thus, a
                    // terminator was not encountered and not all bits have not been consumed.
                    self.consume_bits(num_zeros);
                    self.consume_bits(1);
                    break;
                }
            }

            self.fetch_bits()?;
        }

        Ok(num)
    }

    /// Reads and returns a unary ones encoded integer or an error.
    #[inline(always)]
    fn read_unary_ones(&mut self) -> io::Result<u32> {
        // Note: This algorithm is identical to read_unary_zeros except flipped for 1s.
        let mut num = 0;

        loop {
            let num_ones = self.get_bits().leading_ones();

            if num_ones >= self.num_bits_left() {
                num += self.num_bits_left();
                self.fetch_bits()?;
            }
            else {
                num += num_ones;

                self.consume_bits(num_ones);
                self.consume_bits(1);

                break;
            }
        }

        Ok(num)
    }

    /// Reads and returns a unary ones encoded integer that is capped to a maximum value.
    #[inline(always)]
    fn read_unary_ones_capped(&mut self, mut limit: u32) -> io::Result<u32> {
        // Note: This algorithm is identical to read_unary_zeros_capped except flipped for 1s.
        let mut num = 0;

        loop {
            let num_bits_left = self.num_bits_left();
            let num_ones = min(self.get_bits().leading_ones(), num_bits_left);

            if num_ones >= limit {
                num += limit;
                self.consume_bits(limit);
                break;
            }
            else {
                limit -= num_ones;
                num += num_ones;

                if num_ones < num_bits_left {
                    self.consume_bits(num_ones);
                    self.consume_bits(1);
                    break;
                }
            }

            self.fetch_bits()?;
        }

        Ok(num)
    }

    /// Reads a codebook value from the `BitStream` using the provided `Codebook` and returns the
    /// decoded value or an error.
    #[inline(always)]
    fn read_codebook<E: vlc::CodebookEntry>(
        &mut self,
        codebook: &vlc::Codebook<E>,
    ) -> io::Result<(E::ValueType, u32)> {
        // Attempt to refill the bit buffer with enough bits for the longest codeword in the
        // codebook. However, this does not mean the bit buffer will have enough bits to decode a
        // codeword.
        if self.num_bits_left() < codebook.max_code_len {
            self.fetch_bits_partial()?;
        }

        let mut bits = self.get_bits();

        let bits_per_block = codebook.bits_per_block;
        let bit_shift = u64::BITS - bits_per_block;

        let mut base = 0;

        loop {
            match codebook.table[base + (bits >> bit_shift) as usize] {
                vlc::Entry::Jump { index } => {
                    bits <<= bits_per_block;
                    base = index.into() as usize;
                }
                vlc::Entry::Value { value, code_len } => {
                    let code_len = u32::from(code_len.get());

                    if code_len > self.num_bits_left() {
                        return end_of_bitstream_error();
                    }

                    self.consume_bits(code_len);

                    break Ok((value, code_len));
                }
            }
        }
    }
}

/// `BitStreamLtr` reads bits from most-significant to least-significant from any source
/// that implements [`ReadBytes`].
///
/// Stated another way, if N-bits are read from a `BitReaderLtr` then bit 0, the first bit read,
/// is the most-significant bit, and bit N-1, the last bit read, is the least-significant.
/// is the most-significant bit, and bit N-1, the last bit read, is the least-significant.
pub struct BitReaderLtr<'a> {
    buf: &'a [u8],
    bits: u64,
    n_bits_left: u32,
}

impl<'a> BitReaderLtr<'a> {
    /// Instantiate a new `BitReaderLtr` with the given buffer.
    pub fn new(buf: &'a [u8]) -> Self {
        BitReaderLtr { buf, bits: 0, n_bits_left: 0 }
    }
}

impl private::FetchBitsLtr for BitReaderLtr<'_> {
    #[inline]
    fn fetch_bits_partial(&mut self) -> io::Result<()> {
        let num_bytes = (u64::BITS - self.n_bits_left) as usize >> 3;

        let mut num_bytes_read = 0;

        for &byte in self.buf.iter().take(num_bytes) {
            self.bits |= u64::from(byte) << (u64::BITS - 8 - self.n_bits_left);
            self.n_bits_left += 8;
            num_bytes_read += 1;
        }

        self.buf = &self.buf[num_bytes_read..];

        Ok(())
    }

    fn fetch_bits(&mut self) -> io::Result<()> {
        let read_len = min(self.buf.len(), core::mem::size_of::<u64>());

        if read_len == 0 {
            return end_of_bitstream_error();
        }

        let mut buf = [0u8; core::mem::size_of::<u64>()];

        buf[..read_len].copy_from_slice(&self.buf[..read_len]);

        self.buf = &self.buf[read_len..];

        self.bits = u64::from_be_bytes(buf);
        self.n_bits_left = (read_len as u32) << 3;

        Ok(())
    }

    #[inline(always)]
    fn get_bits(&self) -> u64 {
        self.bits
    }

    #[inline(always)]
    fn num_bits_left(&self) -> u32 {
        self.n_bits_left
    }

    #[inline(always)]
    fn consume_bits(&mut self, num: u32) {
        self.n_bits_left -= num;
        self.bits <<= num;
    }
}

impl ReadBitsLtr for BitReaderLtr<'_> {}

impl FiniteBitStream for BitReaderLtr<'_> {
    fn bits_left(&self) -> u64 {
        (8 * self.buf.len() as u64) + u64::from(self.n_bits_left)
    }
}


