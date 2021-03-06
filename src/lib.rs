#[cfg(test)]
#[macro_use]
extern crate mpeg2ts_reader;
extern crate bitreader;
#[cfg(not(test))]
extern crate mpeg2ts_reader;
#[cfg(test)]
#[macro_use]
extern crate hex_literal;
#[cfg(test)]
#[macro_use]
extern crate matches;

use mpeg2ts_reader::demultiplex;
use mpeg2ts_reader::psi;
use std::fmt;
use std::marker;

#[derive(Debug, PartialEq)]
pub enum EncryptionAlgorithm {
    None,
    DesEcb,
    DesCbc,
    TripleDesEde3Ecb,
    Reserved(u8),
    Private(u8),
}
impl EncryptionAlgorithm {
    pub fn from_id(id: u8) -> EncryptionAlgorithm {
        match id {
            0 => EncryptionAlgorithm::None,
            1 => EncryptionAlgorithm::DesEcb,
            2 => EncryptionAlgorithm::DesCbc,
            3 => EncryptionAlgorithm::TripleDesEde3Ecb,
            _ => {
                if id < 32 {
                    EncryptionAlgorithm::Reserved(id)
                } else {
                    EncryptionAlgorithm::Private(id)
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum SpliceCommandType {
    SpliceNull,
    Reserved(u8),
    SpliceSchedule,
    SpliceInsert,
    TimeSignal,
    BandwidthReservation,
    PrivateCommand,
}
impl SpliceCommandType {
    pub fn from_id(id: u8) -> SpliceCommandType {
        match id {
            0x00 => SpliceCommandType::SpliceNull,
            0x04 => SpliceCommandType::SpliceSchedule,
            0x05 => SpliceCommandType::SpliceInsert,
            0x06 => SpliceCommandType::TimeSignal,
            0x07 => SpliceCommandType::BandwidthReservation,
            0xff => SpliceCommandType::PrivateCommand,
            _ => SpliceCommandType::Reserved(id),
        }
    }
}

pub struct SpliceInfoHeader<'a> {
    buf: &'a [u8],
}
impl<'a> SpliceInfoHeader<'a> {
    const HEADER_LENGTH: usize = 11;

    pub fn new(buf: &'a [u8]) -> (SpliceInfoHeader<'a>, &'a [u8]) {
        if buf.len() < 11 {
            panic!("buffer too short: {} (expected 11)", buf.len());
        }
        let (head, tail) = buf.split_at(11);
        (SpliceInfoHeader { buf: head }, tail)
    }

    pub fn protocol_version(&self) -> u8 {
        self.buf[0]
    }
    pub fn encrypted_packet(&self) -> bool {
        self.buf[1] & 0b1000_0000 != 0
    }
    pub fn encryption_algorithm(&self) -> EncryptionAlgorithm {
        EncryptionAlgorithm::from_id((self.buf[1] & 0b0111_1110) >> 1)
    }
    pub fn pts_adjustment(&self) -> u64 {
        u64::from(self.buf[1] & 1) << 32
            | u64::from(self.buf[2]) << 24
            | u64::from(self.buf[3]) << 16
            | u64::from(self.buf[4]) << 8
            | u64::from(self.buf[5])
    }
    pub fn cw_index(&self) -> u8 {
        self.buf[6]
    }
    pub fn tier(&self) -> u16 {
        u16::from(self.buf[7]) << 4 | u16::from(self.buf[8]) >> 4
    }
    pub fn splice_command_length(&self) -> u16 {
        u16::from(self.buf[8] & 0b00001111) << 8 | u16::from(self.buf[9])
    }
    pub fn splice_command_type(&self) -> SpliceCommandType {
        SpliceCommandType::from_id(self.buf[10])
    }
}
impl<'a> fmt::Debug for SpliceInfoHeader<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "SpliceInfoHeader {{ protocol_version: {:?}, encrypted_packet: {:?}, encryption_algorithm: {:?}, pts_adjustment: {:?}, cw_index: {:?}, tier: {:?} }}",
               self.protocol_version(),
               self.encrypted_packet(),
               self.encryption_algorithm(),
               self.pts_adjustment(),
               self.cw_index(),
               self.tier())
    }
}

#[derive(Debug)]
pub enum SpliceCommand {
    SpliceNull {},
    SpliceInsert {
        splice_event_id: u32,
        reserved: u8,
        splice_detail: SpliceInsert,
    },
    TimeSignal {
        splice_time: SpliceTime
    }
}

#[derive(Debug)]
pub enum NetworkIndicator {
    Out,
    In,
}
impl NetworkIndicator {
    /// panics if `id` is something other than `0` or `1`
    pub fn from_flag(id: u8) -> NetworkIndicator {
        match id {
            0 => NetworkIndicator::In,
            1 => NetworkIndicator::Out,
            _ => panic!(
                "Invalid out_of_network_indicator value: {} (expected 0 or 1)",
                id
            ),
        }
    }
}

#[derive(Debug)]
pub enum SpliceInsert {
    Cancel,
    Insert {
        network_indicator: NetworkIndicator,
        splice_mode: SpliceMode,
        duration: Option<SpliceDuration>,
        unique_program_id: u16,
        avail_num: u8,
        avails_expected: u8,
    },
}

#[derive(Debug)]
pub enum SpliceTime {
    Immediate,
    Timed(Option<u64>),
}

#[derive(Debug)]
pub struct ComponentSplice {
    component_tag: u8,
    splice_time: SpliceTime,
}

#[derive(Debug)]
pub enum SpliceMode {
    Program(SpliceTime),
    Components(Vec<ComponentSplice>),
}

#[derive(Debug)]
pub enum ReturnMode {
    Automatic,
    Manual,
}
impl ReturnMode {
    pub fn from_flag(flag: u8) -> ReturnMode {
        match flag {
            0 => ReturnMode::Manual,
            1 => ReturnMode::Automatic,
            _ => panic!("Invalid auto_return value: {} (expected 0 or 1)", flag),
        }
    }
}

#[derive(Debug)]
pub struct SpliceDuration {
    return_mode: ReturnMode,
    duration: u64,
}

pub trait SpliceInfoProcessor {
    fn process(
        &self,
        header: SpliceInfoHeader,
        command: SpliceCommand,
        descriptors: SpliceDescriptorIter,
    );
}

#[derive(Debug)]
pub enum SpliceDescriptor {
    AvailDescriptor {
        provider_avail_id: u32,
    },
    DTMFDescriptor,         // TODO
    SegmentationDescriptor, // TODO
    TimeDescriptor,         // TODO
    Reserved {
        tag: u8,
        identifier: [u8; 4],
        private_bytes: Vec<u8>,
    },
}
impl SpliceDescriptor {
    fn parse(buf: &[u8]) -> Result<SpliceDescriptor, SpliceDescriptorErr> {
        if buf.len() < 6 {
            return Err(SpliceDescriptorErr::NotEnoughData {
                actual: buf.len(),
                expected: 6,
            });
        }
        let splice_descriptor_tag = buf[0];
        let splice_descriptor_len = buf[1] as usize;
        let splice_descriptor_end = splice_descriptor_len + 2;
        if splice_descriptor_end > buf.len() {
            return Err(SpliceDescriptorErr::NotEnoughData {
                actual: buf.len(),
                expected: splice_descriptor_end,
            });
        }
        let id = &buf[2..6];
        Ok(if id != b"CUEI" {
            SpliceDescriptor::Reserved {
                tag: splice_descriptor_tag,
                identifier: [id[0], id[1], id[2], id[3]],
                private_bytes: buf[6..splice_descriptor_end].to_owned(),
            }
        } else {
            match splice_descriptor_tag {
                0x00 => SpliceDescriptor::AvailDescriptor {
                    provider_avail_id: u32::from(buf[6]) << 24
                        | u32::from(buf[7]) << 16
                        | u32::from(buf[8]) << 8
                        | u32::from(buf[9]),
                },
                0x01 => SpliceDescriptor::DTMFDescriptor,
                0x02 => SpliceDescriptor::SegmentationDescriptor,
                0x03 => SpliceDescriptor::TimeDescriptor,
                _ => SpliceDescriptor::Reserved {
                    tag: splice_descriptor_tag,
                    identifier: [id[0], id[1], id[2], id[3]],
                    private_bytes: buf[6..splice_descriptor_end].to_owned(),
                },
            }
        })
    }
}

#[derive(Debug)]
pub enum SpliceDescriptorErr {
    InvalidDescriptorLength(usize),
    NotEnoughData { expected: usize, actual: usize },
}

pub struct SpliceDescriptorIter<'buf> {
    buf: &'buf [u8],
}
impl<'buf> SpliceDescriptorIter<'buf> {
    fn new(buf: &'buf [u8]) -> SpliceDescriptorIter {
        SpliceDescriptorIter { buf }
    }
}
impl<'buf> Iterator for SpliceDescriptorIter<'buf> {
    type Item = Result<SpliceDescriptor, SpliceDescriptorErr>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buf.is_empty() {
            return None;
        }
        if self.buf.len() < 6 {
            self.buf = &self.buf[0..0];
            return Some(Err(SpliceDescriptorErr::NotEnoughData {
                expected: 2,
                actual: self.buf.len(),
            }));
        }
        let descriptor_length = self.buf[1] as usize;
        if self.buf.len() < descriptor_length + 2 {
            self.buf = &self.buf[0..0];
            return Some(Err(SpliceDescriptorErr::NotEnoughData {
                expected: descriptor_length + 2,
                actual: self.buf.len(),
            }));
        }
        if descriptor_length > 254 {
            self.buf = &self.buf[0..0];
            return Some(Err(SpliceDescriptorErr::InvalidDescriptorLength(
                descriptor_length,
            )));
        }
        let (desc, rest) = self.buf.split_at(usize::from(2 + descriptor_length));
        let result = SpliceDescriptor::parse(desc);
        self.buf = rest;
        Some(result)
    }
}

pub struct Scte35SectionProcessor<P, Ctx: demultiplex::DemuxContext>
where
    P: SpliceInfoProcessor,
{
    processor: P,
    phantom: marker::PhantomData<Ctx>,
}
impl<P, Ctx: demultiplex::DemuxContext> psi::SectionProcessor for Scte35SectionProcessor<P, Ctx>
where
    P: SpliceInfoProcessor,
{
    type Context = Ctx;

    fn start_section<'a>(
        &mut self,
        _ctx: &mut Self::Context,
        header: &psi::SectionCommonHeader,
        data: &'a [u8],
    ) {
        if header.table_id == 0xfc {
            let section_data = &data[psi::SectionCommonHeader::SIZE..];
            if section_data.len() < SpliceInfoHeader::HEADER_LENGTH + 4 {
                println!(
                    "SCTE35: section data too short: {} (must be at least {})",
                    section_data.len(),
                    SpliceInfoHeader::HEADER_LENGTH + 4
                );
                return;
            }
            // trim off the 32-bit CRC, TODO: check the CRC!  (possibly in calling code rather than here?)
            let section_data = &section_data[..section_data.len() - 4];
            let (splice_header, rest) = SpliceInfoHeader::new(section_data);
            //println!("splice header len={}, type={:?}", splice_header.splice_command_length(), splice_header.splice_command_type());
            let command_len = splice_header.splice_command_length() as usize;
            if command_len > rest.len() {
                println!("SCTE35: splice_command_length of {} bytes is too long to fit in remaining {} bytes of section data", command_len, rest.len());
                return;
            }
            let (payload, rest) = rest.split_at(command_len);
            if rest.len() < 2 {
                println!("SCTE35: end of section data while trying to read descriptor_loop_length");
                return;
            }
            let descriptor_loop_length = (u16::from(rest[0]) << 8 | u16::from(rest[1])) as usize;
            if descriptor_loop_length + 2 > rest.len() {
                println!("SCTE35: descriptor_loop_length of {} bytes is too long to fit in remaining {} bytes of section data", descriptor_loop_length, rest.len());
                return;
            }
            let descriptors = &rest[2..2 + descriptor_loop_length];
            let splice_command = match splice_header.splice_command_type() {
                SpliceCommandType::SpliceNull => Some(Self::splice_null(payload)),
                SpliceCommandType::SpliceInsert => Some(Self::splice_insert(payload)),
                SpliceCommandType::TimeSignal => Some(Self::time_signal(payload)),
                _ => None,
            };
            if let Some(splice_command) = splice_command {
                self.processor.process(
                    splice_header,
                    splice_command,
                    SpliceDescriptorIter::new(descriptors),
                );
            } else {
                println!(
                    "SCTE35: unhandled command {:?}",
                    splice_header.splice_command_type()
                );
            }
        } else {
            println!(
                "SCTE35: bad table_id for scte35: {:#x} (expected 0xfc)",
                header.table_id
            );
        }
    }

    fn continue_section<'a>(&mut self, _ctx: &mut Self::Context, _section_data: &'a [u8]) {
        unimplemented!()
    }

    fn reset(&mut self) {
        unimplemented!()
    }
}
impl<P, Ctx: demultiplex::DemuxContext> Scte35SectionProcessor<P, Ctx>
where
    P: SpliceInfoProcessor,
{
    // FIXME: get rid of all unwrap()

    pub fn new(processor: P) -> Scte35SectionProcessor<P, Ctx> {
        Scte35SectionProcessor {
            processor,
            phantom: marker::PhantomData,
        }
    }
    fn splice_null(payload: &[u8]) -> SpliceCommand {
        assert_eq!(0, payload.len());
        SpliceCommand::SpliceNull {}
    }

    fn splice_insert(payload: &[u8]) -> SpliceCommand {
        let mut r = bitreader::BitReader::new(payload);

        let splice_event_id = r.read_u32(32).unwrap();
        let splice_event_cancel_indicator = r.read_bool().unwrap();
        let reserved = r.read_u8(7).unwrap();
        let result = SpliceCommand::SpliceInsert {
            splice_event_id,
            reserved,
            splice_detail: Self::read_splice_detail(&mut r, splice_event_cancel_indicator),
        };
        assert_eq!(r.position() as usize, payload.len() * 8);
        result
    }

    fn time_signal(payload: &[u8]) -> SpliceCommand {
        let mut r = bitreader::BitReader::new(payload);

        let result = SpliceCommand::TimeSignal {
            splice_time: SpliceTime::Timed(Self::read_splice_time(&mut r))
        };
        assert_eq!(r.position() as usize, payload.len() * 8);
        result
    }

    fn read_splice_detail(
        r: &mut bitreader::BitReader,
        splice_event_cancel_indicator: bool,
    ) -> SpliceInsert {
        if splice_event_cancel_indicator {
            SpliceInsert::Cancel
        } else {
            let network_indicator = NetworkIndicator::from_flag(r.read_u8(1).unwrap());
            let program_splice_flag = r.read_bool().unwrap();
            let duration_flag = r.read_bool().unwrap();
            let splice_immediate_flag = r.read_bool().unwrap();
            r.skip(4).unwrap(); // reserved

            SpliceInsert::Insert {
                network_indicator,
                splice_mode: Self::read_splice_mode(r, program_splice_flag, splice_immediate_flag),
                duration: if duration_flag {
                    Some(Self::read_duration(r))
                } else {
                    None
                },
                unique_program_id: r.read_u16(16).unwrap(),
                avail_num: r.read_u8(8).unwrap(),
                avails_expected: r.read_u8(8).unwrap(),
            }
        }
    }

    fn read_splice_mode(
        r: &mut bitreader::BitReader,
        program_splice_flag: bool,
        splice_immediate_flag: bool,
    ) -> SpliceMode {
        if program_splice_flag {
            let time = if splice_immediate_flag {
                SpliceTime::Immediate
            } else {
                SpliceTime::Timed(Self::read_splice_time(r))
            };
            SpliceMode::Program(time)
        } else {
            let component_count = r.read_u8(8).unwrap();
            let compomemts = (0..component_count)
                .map(|_| {
                    let component_tag = r.read_u8(8).unwrap();
                    let splice_time = if splice_immediate_flag {
                        SpliceTime::Immediate
                    } else {
                        SpliceTime::Timed(Self::read_splice_time(r))
                    };
                    ComponentSplice {
                        component_tag,
                        splice_time,
                    }
                }).collect();
            SpliceMode::Components(compomemts)
        }
    }

    fn read_splice_time(r: &mut bitreader::BitReader) -> Option<u64> {
        if r.read_bool().unwrap_or(false) {
            r.skip(6).unwrap(); // reserved
            r.read_u64(33).ok()
        } else {
            r.skip(7).unwrap(); // reserved
            None
        }
    }

    fn read_duration(r: &mut bitreader::BitReader) -> SpliceDuration {
        let return_mode = ReturnMode::from_flag(r.read_u8(1).unwrap());
        r.skip(6).unwrap();
        SpliceDuration {
            return_mode,
            duration: r.read_u64(33).unwrap(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpeg2ts_reader::demultiplex;
    use mpeg2ts_reader::psi;
    use mpeg2ts_reader::psi::SectionProcessor;

    demux_context!(NullDemuxContext, NullStreamConstructor);
    pub struct NullStreamConstructor;
    impl demultiplex::StreamConstructor for NullStreamConstructor {
        type F = demultiplex::NullPacketFilter<NullDemuxContext>;

        fn construct(&mut self, _req: demultiplex::FilterRequest) -> Self::F {
            unimplemented!();
        }
    }

    struct MockSpliceInsertProcessor;
    impl SpliceInfoProcessor for MockSpliceInsertProcessor {
        fn process(
            &self,
            header: SpliceInfoHeader,
            command: SpliceCommand,
            descriptors: SpliceDescriptorIter,
        ) {
            assert_eq!(header.encryption_algorithm(), EncryptionAlgorithm::None);
            assert_matches!(command, SpliceCommand::SpliceInsert{..});
            for d in descriptors {
                d.unwrap();
            }
        }
    }

    #[test]
    fn it_works() {
        let data = hex!(
            "fc302500000000000000fff01405000000017feffe2d142b00fe0123d3080001010100007f157a49"
        );
        let mut parser = Scte35SectionProcessor::new(MockSpliceInsertProcessor);
        let header = psi::SectionCommonHeader::new(&data[..psi::SectionCommonHeader::SIZE]);
        let mut ctx = NullDemuxContext::new(NullStreamConstructor);
        parser.start_section(&mut ctx, &header, &data[..]);
    }

    struct MockTimeSignalProcessor;
    impl SpliceInfoProcessor for MockTimeSignalProcessor {
        fn process(
            &self,
            header: SpliceInfoHeader,
            command: SpliceCommand,
            descriptors: SpliceDescriptorIter,
        ) {
            assert_eq!(header.encryption_algorithm(), EncryptionAlgorithm::None);
            assert_matches!(command, SpliceCommand::TimeSignal{..});
            for d in descriptors {
                d.unwrap();
            }
        }
    }

    #[test]
    fn it_understands_time_signal() {
        let data = hex!(
            "fc302700000000000000fff00506ff592d03c00011020f43554549000000017fbf000010010112ce0e6b"
        );
        let mut parser = Scte35SectionProcessor::new(MockTimeSignalProcessor);
        let header = psi::SectionCommonHeader::new(&data[..psi::SectionCommonHeader::SIZE]);
        let mut ctx = NullDemuxContext::new(NullStreamConstructor);
        parser.start_section(&mut ctx, &header, &data[..]);
    }

    #[test]
    fn splice_descriptor() {
        let data = [];
        assert_matches!(
            SpliceDescriptor::parse(&data[..]),
            Err(SpliceDescriptorErr::NotEnoughData { .. })
        );
        let data = hex!("01084D5949440000"); // descriptor payload too short
        assert_matches!(
            SpliceDescriptor::parse(&data[..]),
            Err(SpliceDescriptorErr::NotEnoughData { .. })
        );
        let data = hex!("01084D59494400000003");
        assert_matches!(
            SpliceDescriptor::parse(&data[..]),
            Ok(SpliceDescriptor::Reserved {
                tag: 01,
                identifier: [0x4D, 0x59, 0x49, 0x44],
                private_bytes: _,
            })
        );
    }
}
