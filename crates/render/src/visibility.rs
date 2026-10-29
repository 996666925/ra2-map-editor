//! Immutable bounds tree. Queries return the original painter order, so alpha
//! blending and equal-depth ties do not change when the camera moves.
use egui::Rect;

struct Entry {
    bounds: Rect,
    id: u32,
}
struct Node {
    bounds: Rect,
    entries: std::ops::Range<usize>,
    children: Option<[usize; 2]>,
}
pub(crate) struct VisibilityIndex {
    entries: Vec<Entry>,
    nodes: Vec<Node>,
}
impl VisibilityIndex {
    pub fn new(bounds: impl Iterator<Item = Rect>) -> Self {
        let mut index = Self {
            entries: bounds
                .enumerate()
                .map(|(id, bounds)| Entry {
                    bounds,
                    id: id as u32,
                })
                .collect(),
            nodes: Vec::new(),
        };
        if !index.entries.is_empty() {
            index.build(0..index.entries.len());
        }
        index
    }
    fn build(&mut self, range: std::ops::Range<usize>) -> usize {
        let bounds = self.entries[range.clone()]
            .iter()
            .fold(Rect::NOTHING, |b, e| b.union(e.bounds));
        let node = self.nodes.len();
        self.nodes.push(Node {
            bounds,
            entries: range.clone(),
            children: None,
        });
        if range.len() > 16 {
            let middle = range.start + range.len() / 2;
            let axis = usize::from(bounds.height() > bounds.width());
            self.entries[range.clone()].select_nth_unstable_by(middle - range.start, |a, b| {
                a.bounds.center()[axis].total_cmp(&b.bounds.center()[axis])
            });
            let children = [
                self.build(range.start..middle),
                self.build(middle..range.end),
            ];
            self.nodes[node].children = Some(children);
        }
        node
    }
    pub fn visible(&self, viewport: Rect) -> Vec<u32> {
        let mut ids = Vec::new();
        if !self.nodes.is_empty() {
            self.query(0, viewport, &mut ids);
        }
        ids.sort_unstable();
        ids
    }
    fn query(&self, node: usize, viewport: Rect, ids: &mut Vec<u32>) {
        let node = &self.nodes[node];
        if !node.bounds.intersects(viewport) {
            return;
        }
        if viewport.contains_rect(node.bounds) {
            ids.extend(self.entries[node.entries.clone()].iter().map(|e| e.id));
        } else if let Some(children) = node.children {
            for child in children {
                self.query(child, viewport, ids);
            }
        } else {
            ids.extend(
                self.entries[node.entries.clone()]
                    .iter()
                    .filter(|e| e.bounds.intersects(viewport))
                    .map(|e| e.id),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn large_map_queries_match_full_scan_at_edges_zoom_and_negative_coordinates() {
        // Two projected tiles per map cell, plus tall art and detached shadows.
        let mut bounds: Vec<_> = (0..512)
            .flat_map(|y| {
                (0..256).map(move |x| {
                    Rect::from_min_size(
                        egui::pos2((x - y) as f32 * 30., (x + y) as f32 * 15.),
                        egui::vec2(60., 30.),
                    )
                })
            })
            .collect();
        bounds.extend([
            Rect::from_min_max(egui::pos2(-900., -500.), egui::pos2(-850., 100.)),
            Rect::from_min_max(egui::pos2(-1000., -600.), egui::pos2(-600., -200.)),
        ]);
        let index = VisibilityIndex::new(bounds.iter().copied());
        for center in [
            egui::pos2(-870., 0.),
            egui::pos2(-600., -200.),
            egui::pos2(0., 9000.),
            egui::pos2(50000., 50000.),
        ] {
            for zoom in [0.02, 0.25, 1., 8.] {
                let viewport = Rect::from_center_size(center, egui::vec2(1280., 800.) / zoom);
                let expected: Vec<_> = bounds
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| b.intersects(viewport))
                    .map(|(i, _)| i as u32)
                    .collect();
                assert_eq!(index.visible(viewport), expected);
            }
        }
        let viewport = Rect::from_center_size(egui::pos2(0., 6000.), egui::vec2(1280., 800.));
        let visible = index.visible(viewport).len();
        assert!(
            visible > 100 && visible < bounds.len() / 50,
            "visible={visible}, total={}",
            bounds.len()
        );
        assert!(
            VisibilityIndex::new(std::iter::empty())
                .visible(viewport)
                .is_empty()
        );
    }
}
