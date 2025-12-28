//! Debug hex dump utility

/// Print a hex dump of memory
pub fn hexdump(memory: &[u8]) {
    let mut all_zero = 0;
    let mut all_one = 0;

    for i in (0..memory.len()).step_by(16) {
        all_zero += 1;
        all_one += 1;

        // Check if line is all zeros
        let line = &memory[i..std::cmp::min(i + 16, memory.len())];
        if line.iter().all(|&b| b == 0) {
            // Keep counting
        } else {
            all_zero = 0;
        }

        // Check if line is all 0xff
        if line.iter().all(|&b| b == 0xff) {
            // Keep counting
        } else {
            all_one = 0;
        }

        if all_zero < 2 && all_one < 2 {
            print!("{:08x}:", i);

            // Print hex bytes
            for j in 0..16 {
                if i + j < memory.len() {
                    print!(" {:02x}", memory[i + j]);
                } else {
                    print!("   ");
                }
            }

            print!("  ");

            // Print ASCII
            for j in 0..16 {
                if i + j < memory.len() {
                    let c = memory[i + j];
                    if c.is_ascii_graphic() || c == b' ' {
                        print!("{}", c as char);
                    } else {
                        print!(".");
                    }
                }
            }

            println!();
        } else if all_zero == 2 || all_one == 2 {
            println!("...");
        }
    }
}
