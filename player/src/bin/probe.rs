use std::{
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
    time::Duration,
};

use clap::Parser;
use container::{EncodableData, PacketDataType, SubRect, SubRectVec};
use player::{FormatDuration, Reader};

#[derive(clap::Parser)]
struct ProbeArgs {
    #[arg(long)]
    seektables: Option<PathBuf>,
    #[arg(long)]
    inspect_packets: bool,
    #[arg(long)]
    debug_subtitles: bool,
    #[arg(long)]
    extract_header_xer: Option<PathBuf>,
    #[arg(long)]
    extract_header_der: Option<PathBuf>,
    input: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let cli = ProbeArgs::parse();

    let input = BufReader::new(File::open(&cli.input)?);

    let reader = Reader::new(input);

    let (reader, header) = reader.read_header()?;
    println!("Header: \n{header:#?}");

    if let Some(path) = cli.extract_header_xer {
        std::fs::write(path, rasn::xer::encode(&header).unwrap())?;
    }

    if let Some(path) = cli.extract_header_der {
        std::fs::write(path, rasn::der::encode(&header).unwrap())?;
    }

    let (mut reader, seektables) = reader.read_seektables()?;

    if let Some(seektable_path) = cli.seektables {
        println!("Decoding seektable to {}...", seektable_path.display());

        let mut seektable_debug = BufWriter::new(File::create(seektable_path)?);

        for (stream, seektable) in seektables {
            writeln!(seektable_debug, "Seek Table <-> Stream {stream}")?;

            for row in seektable {
                writeln!(
                    seektable_debug,
                    "{} -> byte {}",
                    FormatDuration(Duration::from_micros(row.ts as u64)),
                    row.location
                )?;
            }
        }
    }

    if !cli.inspect_packets && !cli.debug_subtitles {
        return Ok(());
    }

    while let Ok((packet_header, data)) = reader.read_packet() {
        if cli.inspect_packets {
            println!("{packet_header}");
        }

        if cli.debug_subtitles && packet_header.data_type == PacketDataType::Subtitle {
            println!("Subtitles for stream {} ->", packet_header.stream);
            let new_subs: Vec<SubRect> = SubRectVec::decode_from(&mut data.as_slice())
                .unwrap()
                .into_inner();
            println!("{new_subs:#?}");
        }
    }

    // let reader = std::thread::spawn(move || -> anyhow::Result<()> {

    //     while let Ok(packet_header) = Packet::decode_from(&mut input) {
    //         let mut send_slot = packet_tx.send_ref().unwrap();
    //         send_slot.header = packet_header;
    //         send_slot.data.resize(len, 0);
    //     }
    // }
    Ok(())
}
