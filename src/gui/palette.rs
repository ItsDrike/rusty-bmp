use bmp::raw::Bmp;

pub fn extract_palette_colors(bmp: &Bmp) -> Vec<[u8; 4]> {
    match bmp {
        Bmp::Core(data) => data.color_table.iter().map(|c| [c.red, c.green, c.blue, 255]).collect(),
        Bmp::Info(data) => data.color_table.iter().map(|c| [c.red, c.green, c.blue, 255]).collect(),
        Bmp::V4(data) => data.color_table.iter().map(|c| [c.red, c.green, c.blue, 255]).collect(),
        Bmp::V5(data) => data.color_table.iter().map(|c| [c.red, c.green, c.blue, 255]).collect(),
    }
}
