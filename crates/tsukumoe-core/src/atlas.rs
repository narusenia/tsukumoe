// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Rectangle packing for texture atlases (M5-03a): which page and where each
//! image goes. It knows sizes only, so it is the same for every image format;
//! `tsukumoe-format` draws the pages and moves the UVs.
//!
//! The method is MaxRects with best short side fit: each image goes where it
//! leaves the least room on its short side. Bigger images go first. The same
//! input gives the same packing.

/// Where an image sits: the page, and the top left of the image itself (the
/// padding around it is outside this corner).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub page: usize,
    pub x: u32,
    pub y: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packing {
    /// The size of each page: a power of two on each side, at most `max_size`.
    pub pages: Vec<(u32, u32)>,
    /// One per input image; `None` for one that cannot be placed (empty, or
    /// bigger than a page with its padding).
    pub slots: Vec<Option<Placement>>,
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

impl Rect {
    fn right(&self) -> u32 {
        self.x + self.w
    }

    fn bottom(&self) -> u32 {
        self.y + self.h
    }

    fn contains(&self, o: &Rect) -> bool {
        self.x <= o.x && self.y <= o.y && o.right() <= self.right() && o.bottom() <= self.bottom()
    }

    fn overlaps(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }
}

struct Page {
    free: Vec<Rect>,
    /// How far the used area reaches.
    extent: (u32, u32),
}

impl Page {
    fn new(size: u32) -> Self {
        Self {
            free: vec![Rect {
                x: 0,
                y: 0,
                w: size,
                h: size,
            }],
            extent: (0, 0),
        }
    }

    /// The best free spot for a `w` by `h` rectangle: the least leftover on
    /// the short side, then on the long side.
    fn find(&self, w: u32, h: u32) -> Option<Rect> {
        self.free
            .iter()
            .filter(|r| r.w >= w && r.h >= h)
            .min_by_key(|r| {
                let (dw, dh) = (r.w - w, r.h - h);
                (dw.min(dh), dw.max(dh), r.y, r.x)
            })
            .map(|r| Rect {
                x: r.x,
                y: r.y,
                w,
                h,
            })
    }

    fn place(&mut self, used: Rect) {
        let mut next = Vec::with_capacity(self.free.len() + 4);
        for r in &self.free {
            if !r.overlaps(&used) {
                next.push(*r);
                continue;
            }
            if used.x > r.x {
                next.push(Rect {
                    w: used.x - r.x,
                    ..*r
                });
            }
            if used.right() < r.right() {
                next.push(Rect {
                    x: used.right(),
                    w: r.right() - used.right(),
                    ..*r
                });
            }
            if used.y > r.y {
                next.push(Rect {
                    h: used.y - r.y,
                    ..*r
                });
            }
            if used.bottom() < r.bottom() {
                next.push(Rect {
                    y: used.bottom(),
                    h: r.bottom() - used.bottom(),
                    ..*r
                });
            }
        }
        // Drop what another free rectangle already covers.
        let mut keep = vec![true; next.len()];
        for i in 0..next.len() {
            for j in 0..next.len() {
                if i != j
                    && keep[j]
                    && next[j].contains(&next[i])
                    && (next[i].w, next[i].h) != (next[j].w, next[j].h)
                {
                    keep[i] = false;
                    break;
                }
            }
        }
        let mut seen: Vec<Rect> = Vec::new();
        self.free = next
            .into_iter()
            .zip(keep)
            .filter(|(_, k)| *k)
            .map(|(r, _)| r)
            .filter(|r| {
                // Equal rectangles collapse to one.
                if seen
                    .iter()
                    .any(|s| (s.x, s.y, s.w, s.h) == (r.x, r.y, r.w, r.h))
                {
                    false
                } else {
                    seen.push(*r);
                    true
                }
            })
            .collect();
        self.extent = (
            self.extent.0.max(used.right()),
            self.extent.1.max(used.bottom()),
        );
    }
}

fn cover(extent: u32, max: u32) -> u32 {
    extent.max(1).next_power_of_two().min(max)
}

/// Packs images of the given `(width, height)` into pages of at most
/// `max_size` on a side, `padding` pixels apart from each other and from the
/// page's edge.
pub fn pack(items: &[(u32, u32)], max_size: u32, padding: u32) -> Packing {
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|&i| {
        let (w, h) = items[i];
        (
            std::cmp::Reverse(w.max(h)),
            std::cmp::Reverse(u64::from(w) * u64::from(h)),
            i,
        )
    });
    let mut pages: Vec<Page> = Vec::new();
    let mut slots = vec![None; items.len()];
    for i in order {
        let (w, h) = items[i];
        let (pw, ph) = (
            u64::from(w) + 2 * u64::from(padding),
            u64::from(h) + 2 * u64::from(padding),
        );
        if w == 0 || h == 0 || pw > u64::from(max_size) || ph > u64::from(max_size) {
            continue;
        }
        let (pw, ph) = (pw as u32, ph as u32);
        let spot = pages
            .iter()
            .enumerate()
            .find_map(|(n, p)| p.find(pw, ph).map(|r| (n, r)));
        let (n, rect) = match spot {
            Some(found) => found,
            None => {
                pages.push(Page::new(max_size));
                let n = pages.len() - 1;
                let rect = pages[n]
                    .find(pw, ph)
                    .expect("an image that fits a page fits an empty one");
                (n, rect)
            }
        };
        pages[n].place(rect);
        slots[i] = Some(Placement {
            page: n,
            x: rect.x + padding,
            y: rect.y + padding,
        });
    }
    Packing {
        pages: pages
            .iter()
            .map(|p| (cover(p.extent.0, max_size), cover(p.extent.1, max_size)))
            .collect(),
        slots,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every image is inside its page, and no two images (with their padding)
    /// overlap.
    fn check(items: &[(u32, u32)], max: u32, padding: u32, packing: &Packing) {
        let mut taken: Vec<(usize, Rect)> = Vec::new();
        for (i, slot) in packing.slots.iter().enumerate() {
            let Some(s) = slot else { continue };
            let (w, h) = items[i];
            let (pw, ph) = packing.pages[s.page];
            assert!(s.x >= padding && s.y >= padding, "{i} touches the edge");
            assert!(
                s.x + w + padding <= pw && s.y + h + padding <= ph,
                "{i} leaves its page"
            );
            assert!(pw <= max && ph <= max);
            let r = Rect {
                x: s.x - padding,
                y: s.y - padding,
                w: w + 2 * padding,
                h: h + 2 * padding,
            };
            for (page, other) in &taken {
                assert!(*page != s.page || !r.overlaps(other), "{i} overlaps");
            }
            taken.push((s.page, r));
        }
    }

    #[test]
    fn images_are_packed_without_overlap_and_inside_their_pages() {
        let items: Vec<(u32, u32)> = (0..60)
            .map(|i| (10 + (i * 37) % 90, 8 + (i * 53) % 70))
            .collect();
        for padding in [0, 1, 2, 4] {
            let packing = pack(&items, 256, padding);
            check(&items, 256, padding, &packing);
            assert!(packing.slots.iter().all(Option::is_some));
        }
    }

    #[test]
    fn many_small_images_take_fewer_pages_than_images() {
        let items = vec![(30, 30); 100];
        let packing = pack(&items, 128, 2);
        check(&items, 128, 2, &packing);
        assert!(packing.pages.len() < 100 / 4, "{}", packing.pages.len());
    }

    #[test]
    fn pages_are_the_smallest_powers_of_two_that_cover_what_is_used() {
        let packing = pack(&[(20, 10)], 4096, 0);
        assert_eq!(packing.pages, [(32, 16)]);
        assert_eq!(
            packing.slots,
            [Some(Placement {
                page: 0,
                x: 0,
                y: 0
            })]
        );
        // Padding counts toward the cover.
        assert_eq!(pack(&[(30, 30)], 4096, 2).pages, [(64, 64)]);
    }

    #[test]
    fn an_image_that_does_not_fit_a_page_is_left_out() {
        let items = [(100, 100), (300, 10), (0, 5), (254, 254)];
        let packing = pack(&items, 256, 2);
        assert!(packing.slots[0].is_some());
        assert!(packing.slots[1].is_none(), "wider than a page");
        assert!(packing.slots[2].is_none(), "empty");
        assert!(packing.slots[3].is_none(), "with its padding it is 258");
        check(&items, 256, 2, &packing);
    }

    #[test]
    fn the_same_input_gives_the_same_packing() {
        let items: Vec<(u32, u32)> = (0..40).map(|i| (5 + i * 3 % 50, 7 + i * 5 % 40)).collect();
        assert_eq!(pack(&items, 128, 1), pack(&items, 128, 1));
    }

    #[test]
    fn nothing_to_pack_gives_no_pages() {
        let packing = pack(&[], 256, 2);
        assert!(packing.pages.is_empty() && packing.slots.is_empty());
    }

    #[test]
    fn a_full_page_is_filled_exactly() {
        // Four quarters fill one page with no padding.
        let packing = pack(&[(64, 64); 4], 128, 0);
        assert_eq!(packing.pages, [(128, 128)]);
        check(&[(64, 64); 4], 128, 0, &packing);
    }
}
