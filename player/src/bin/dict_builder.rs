use std::{
    fs::File,
    io::{BufReader, Write},
    path::PathBuf,
};

use clap::Parser;
use container::PacketDataType;
use humansize::format_size;
use parse_size::parse_size;
use player::Reader;

#[derive(clap::Parser, Debug, Clone)]
#[command()]
struct DictArgs {
    #[arg(short, long)]
    input: PathBuf,
    #[arg(short, long)]
    output: PathBuf,
    #[arg(long, default_value_t = 2usize.pow(30) * 4, value_parser = |s: &str| parse_size(s).map(|v| v as usize))]
    mem_usage: usize,
    #[arg(long, default_value_t = 112_000, value_parser = |s: &str| parse_size(s).map(|v| v as usize))]
    dict_size: usize,
}

fn main() -> anyhow::Result<()> {
    let args = DictArgs::parse();

    let input = BufReader::new(File::open(args.input)?);

    let reader = Reader::new(input);

    let (reader, _) = reader.read_header()?;
    let (mut reader, _) = reader.read_seektables()?;

    let mut data_buffer = Vec::with_capacity(args.mem_usage);
    let mut data_sizes = Vec::with_capacity(256_000);

    while let Ok((packet_header, mut data)) = reader.read_packet() {
        if data_buffer.len() >= args.mem_usage {
            break;
        }

        if packet_header.data_type != PacketDataType::Video {
            continue;
        }

        print!("\x1b[2K\r");
        print!(
            "Read {}",
            format_size(data_buffer.len(), humansize::DECIMAL)
        );
        std::io::stdout().flush()?;

        data_sizes.push(data.len());
        data_buffer.append(&mut data);
    }

    println!();

    println!("Training dictionary...");
    let dict = zstd::dict::from_continuous(&data_buffer, &data_sizes, args.dict_size)?;
    println!("done!");
    std::fs::write(args.output, dict)?;

    // let reader = std::thread::spawn(move || -> anyhow::Result<()> {

    //     while let Ok(packet_header) = Packet::decode_from(&mut input) {
    //         let mut send_slot = packet_tx.send_ref().unwrap();
    //         send_slot.header = packet_header;
    //         send_slot.data.resize(len, 0);
    //     }
    // }
    Ok(())
}
