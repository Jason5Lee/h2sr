use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
};

fn main() -> anyhow::Result<()> {
    let mut reader = BufReader::new(File::open("test.txt")?);
    let mut proxy_writer = BufWriter::new(File::create("proxy.txt")?);
    let mut white_writer = BufWriter::new(File::create("white.txt")?);
    let mut line = String::new();
    let mut line_num: u32 = 0;
    loop {
        line_num += 1;
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let mut line: &str = &line[..(line.len() - 1)];

        if line == "!##############General List End#################" {
            break;
        }
        if line.starts_with('!') {
            continue;
        }
        if line.starts_with("[AutoProxy") {
            continue;
        }
        if line == r#"/^https?:\/\/[^\/]+blogspot\.(.*)/"# {
            // TODO: blogspot.com
            continue;
        }
        if line
            == r#"/^https?:\/\/([^\/]+\.)*google\.(ac|ad|ae|af|ai|al|am|as|at|az|ba|be|bf|bg|bi|bj|bs|bt|by|ca|cat|cd|cf|cg|ch|ci|cl|cm|co.ao|co.bw|co.ck|co.cr|co.id|co.il|co.in|co.jp|co.ke|co.kr|co.ls|co.ma|com|com.af|com.ag|com.ai|com.ar|com.au|com.bd|com.bh|com.bn|com.bo|com.br|com.bz|com.co|com.cu|com.cy|com.do|com.ec|com.eg|com.et|com.fj|com.gh|com.gi|com.gt|com.hk|com.jm|com.kh|com.kw|com.lb|com.ly|com.mm|com.mt|com.mx|com.my|com.na|com.nf|com.ng|com.ni|com.np|com.om|com.pa|com.pe|com.pg|com.ph|com.pk|com.pr|com.py|com.qa|com.sa|com.sb|com.sg|com.sl|com.sv|com.tj|com.tr|com.tw|com.ua|com.uy|com.vc|com.vn|co.mz|co.nz|co.th|co.tz|co.ug|co.uk|co.uz|co.ve|co.vi|co.za|co.zm|co.zw|cv|cz|de|dj|dk|dm|dz|ee|es|eu|fi|fm|fr|ga|ge|gg|gl|gm|gp|gr|gy|hk|hn|hr|ht|hu|ie|im|iq|is|it|it.ao|je|jo|kg|ki|kz|la|li|lk|lt|lu|lv|md|me|mg|mk|ml|mn|ms|mu|mv|mw|mx|ne|nl|no|nr|nu|org|pl|pn|ps|pt|ro|rs|ru|rw|sc|se|sh|si|sk|sm|sn|so|sr|st|td|tg|tk|tl|tm|tn|to|tt|us|vg|vn|vu|ws)\/.*/"#
        {
            // TODO: google.<pattern>
            continue;
        }

        let writer = if let Some(new_line) = line.strip_prefix("@@") {
            line = new_line;
            &mut white_writer
        } else {
            &mut proxy_writer
        };

        if let Some(new_line) = line.strip_prefix("||") {
            line = new_line;
        }
        if let Some(new_line) = line.strip_prefix("|http://") {
            line = new_line;
        }
        if let Some(new_line) = line.strip_prefix("http://") {
            line = new_line;
        }
        if let Some(new_line) = line.strip_prefix("|https://") {
            line = new_line;
        }
        if let Some(path) = line.find('/') {
            line = &line[..path];
        }
        if let Some(path) = line.find("%2F") {
            line = &line[..path];
        }
        if let Some(star) = line.find('*') {
            line = &line[(star + 1)..];
        }

        if line
            .chars()
            .all(|ch| ch.is_numeric() || ch == '.' || ch == ':')
        {
            continue; // IP
        }

        if !line
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
        {
            println!("{}: {}", line_num, line);
            break;
        }
        if line.starts_with('.') {
            line = &line[1..];
        }
        writeln!(writer, "\"{}\",", line)?;
    }
    Ok(())
}
