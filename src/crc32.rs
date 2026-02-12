const POLYNOMIAL: u32 = 0xedb88320;
const SEED: u32 = 0xffffffff;

fn init_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for i in 0..256u32 {
        let mut entry = i;
        for _ in 0..8 {
            if entry & 1 == 1 {
                entry = (entry >> 1) ^ POLYNOMIAL;
            } else {
                entry >>= 1;
            }
        }
        table[i as usize] = entry;
    }
    table
}

pub fn compute(buffer: &[u8]) -> u32 {
    let table = init_table();
    let mut hash = SEED;
    for &b in buffer {
        hash = (hash >> 8) ^ table[(b ^ (hash as u8)) as usize];
    }
    !hash
}
