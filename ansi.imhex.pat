/*

File Format!

-- (marker: len_bytes, u64) Header: DER-encoded FormatData
-- (marker: amount of seektables, u8) Seek Tables
    -- (stream_index: u16)
    -- (seek_table_length: u64 / bytes)
-- (interleaved packet data)

*/


struct Header {
    le u64 headerLen;
    u8 header[headerLen];
};

struct Seektable {
    u8 streamIndex;
    le u64 tableLen;
    u8 table[tableLen];
};

struct SeektableSection {
    u8 sectionLen;
    Seektable tables[sectionLen];
};

struct SideDataPair {
    char key[4];
    u8 dataLen;
    u8 data[dataLen];
};

struct SideData {
    u8 tableLen;
    SideDataPair table[tableLen];
};

Header header @ 0x00;
SeektableSection seektables @ $;