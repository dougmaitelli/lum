//! Kernel-reported VRAM pools (bytes). In particular, GTT/system RAM is not VRAM.
use std::{fs, path::Path};

pub struct Vram {
    pub card: String,
    pub used: u64,
    pub total: u64,
}
impl Vram {
    pub fn summary(&self) -> String {
        const GIB: f64 = (1u64 << 30) as f64;
        format!(
            "{:.1} / {:.1} GiB ({:.0}%)",
            self.used as f64 / GIB,
            self.total as f64 / GIB,
            self.used as f64 * 100.0 / self.total as f64
        )
    }
}
pub fn read_vram(root: &Path) -> Vec<Vram> {
    let mut cards: Vec<_> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let card = entry.file_name().into_string().ok()?;
            let index = card.strip_prefix("card")?;
            if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let device = entry.path().join("device");
            let bytes = |name: &str| {
                fs::read_to_string(device.join(name))
                    .ok()?
                    .trim()
                    .parse::<u64>()
                    .ok()
            };
            let used = bytes("mem_info_vram_used")?;
            let total = bytes("mem_info_vram_total")?;
            (total > 0 && used <= total).then_some(Vram { card, used, total })
        })
        .collect();
    cards.sort_by(|a, b| a.card.cmp(&b.card));
    cards
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_vram_once_per_card_and_skips_unavailable_counters() {
        let tmp = tempfile::tempdir().unwrap();
        for (card, used, total) in [
            ("card2", "2147483648", "8589934592"),
            ("renderD128", "1", "4"),
            ("card2-DP-1", "1", "4"),
            ("card0", "0", "0"),
            ("card1", "?", "123"),
        ] {
            let d = tmp.path().join(card).join("device");
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("mem_info_vram_used"), used).unwrap();
            fs::write(d.join("mem_info_vram_total"), total).unwrap();
        }
        let cards = read_vram(tmp.path());
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].card, "card2");
        assert_eq!(cards[0].summary(), "2.0 / 8.0 GiB (25%)");
    }
}
