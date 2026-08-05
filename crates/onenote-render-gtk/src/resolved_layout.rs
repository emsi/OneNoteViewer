use onenote_core::{ObjectId, Rect};
use onenote_render::{
    HitRegion, PageScene, SceneFlowId, SceneFlowPosition, SceneNode, SceneNodeId,
};
use std::collections::{BTreeMap, HashMap};

/// Adapter-specific node geometry after exact text and math measurement.
pub(crate) struct ResolvedLayout {
    pub(crate) bounds: Rect,
    node_bounds: HashMap<SceneNodeId, Rect>,
}

impl ResolvedLayout {
    pub(crate) fn new(scene: &PageScene, measured_heights: &HashMap<SceneNodeId, f32>) -> Self {
        let groups = flow_groups(scene);
        let mut group_offsets = HashMap::new();
        let mut groups = groups.into_values().collect::<Vec<_>>();
        groups.sort_by_key(|group| std::cmp::Reverse(group.depth));
        let no_measurements = HashMap::new();

        for group in groups {
            let mut previous: Option<(Rect, Rect)> = None;
            for position in group.positions.into_values() {
                let authored = item_bounds(scene, &position, &no_measurements, &HashMap::new());
                let resolved = item_bounds(scene, &position, measured_heights, &group_offsets);
                let offset = previous.map_or(0.0, |(previous_authored, previous_resolved)| {
                    let authored_gap = authored.y - bottom(previous_authored);
                    bottom(previous_resolved) + authored_gap - resolved.y
                });
                group_offsets.insert(position, offset);
                previous = Some((authored, translate_y(resolved, offset)));
            }
        }

        let node_bounds = scene
            .nodes
            .iter()
            .map(|node| {
                (
                    node.id.clone(),
                    resolved_node_bounds(node, measured_heights, &group_offsets),
                )
            })
            .collect::<HashMap<_, _>>();
        let bounds = node_bounds
            .values()
            .copied()
            .fold(scene.bounds, Rect::union);

        Self {
            bounds,
            node_bounds,
        }
    }

    pub(crate) fn node_bounds(&self, node: &SceneNode) -> Rect {
        self.node_bounds
            .get(&node.id)
            .copied()
            .unwrap_or(node.bounds)
    }

    pub(crate) fn visible_nodes<'a>(
        &'a self,
        scene: &'a PageScene,
        viewport: Rect,
        overscan: f32,
    ) -> impl Iterator<Item = &'a SceneNode> + 'a {
        let viewport = expanded(viewport, overscan);
        scene
            .nodes
            .iter()
            .filter(move |node| intersects(self.node_bounds(node), viewport))
    }

    pub(crate) fn hit_region_bounds(&self, node: &SceneNode, region: &HitRegion) -> Rect {
        let resolved = self.node_bounds(node);
        Rect {
            x: region.bounds.x + resolved.x - node.bounds.x,
            y: region.bounds.y + resolved.y - node.bounds.y,
            ..region.bounds
        }
    }

    pub(crate) fn source_bounds(&self, scene: &PageScene, source: &ObjectId) -> Option<Rect> {
        scene
            .nodes
            .iter()
            .filter(|node| node.source_object_id == *source)
            .map(|node| self.node_bounds(node))
            .reduce(Rect::union)
    }
}

struct FlowGroup {
    depth: usize,
    positions: BTreeMap<u32, SceneFlowPosition>,
}

fn flow_groups(scene: &PageScene) -> HashMap<SceneFlowId, FlowGroup> {
    let mut groups = HashMap::new();
    for node in &scene.nodes {
        for (depth, position) in node.flow_path.iter().enumerate() {
            let group = groups
                .entry(position.group.clone())
                .or_insert_with(|| FlowGroup {
                    depth,
                    positions: BTreeMap::new(),
                });
            group.depth = group.depth.max(depth);
            group
                .positions
                .entry(position.order)
                .or_insert_with(|| position.clone());
        }
    }
    groups
}

fn item_bounds(
    scene: &PageScene,
    position: &SceneFlowPosition,
    measured_heights: &HashMap<SceneNodeId, f32>,
    offsets: &HashMap<SceneFlowPosition, f32>,
) -> Rect {
    scene
        .nodes
        .iter()
        .filter_map(|node| {
            let index = node
                .flow_path
                .iter()
                .position(|candidate| candidate == position)?;
            let mut bounds = measured_node_bounds(node, measured_heights);
            bounds.y += node.flow_path[index + 1..]
                .iter()
                .filter_map(|nested| offsets.get(nested))
                .sum::<f32>();
            Some(bounds)
        })
        .reduce(Rect::union)
        .unwrap_or_default()
}

fn resolved_node_bounds(
    node: &SceneNode,
    measured_heights: &HashMap<SceneNodeId, f32>,
    offsets: &HashMap<SceneFlowPosition, f32>,
) -> Rect {
    let mut bounds = measured_node_bounds(node, measured_heights);
    bounds.y += node
        .flow_path
        .iter()
        .filter_map(|position| offsets.get(position))
        .sum::<f32>();
    bounds
}

fn measured_node_bounds(node: &SceneNode, measured_heights: &HashMap<SceneNodeId, f32>) -> Rect {
    let height = measured_heights
        .get(&node.id)
        .copied()
        .filter(|height| height.is_finite() && *height > 0.0)
        .unwrap_or(node.bounds.height);
    Rect {
        height,
        ..node.bounds
    }
}

fn translate_y(mut bounds: Rect, offset: f32) -> Rect {
    bounds.y += offset;
    bounds
}

fn bottom(bounds: Rect) -> f32 {
    bounds.y + bounds.height
}

fn expanded(rect: Rect, amount: f32) -> Rect {
    let amount = amount.max(0.0);
    Rect {
        x: rect.x - amount,
        y: rect.y - amount,
        width: rect.width + amount * 2.0,
        height: rect.height + amount * 2.0,
    }
}

fn intersects(left: Rect, right: Rect) -> bool {
    left.x <= right.x + right.width
        && left.x + left.width >= right.x
        && left.y <= right.y + right.height
        && left.y + left.height >= right.y
}

#[cfg(test)]
mod tests {
    use super::ResolvedLayout;
    use onenote_core::{Color, ObjectId, PageId, Rect};
    use onenote_render::{
        AccessibilityRole, AccessibilitySemantics, HitAction, HitRegion, PageScene, SceneFlowId,
        SceneFlowPosition, SceneNode, SceneNodeId, ScenePrimitive,
    };
    use std::collections::HashMap;

    #[test]
    fn preserves_authored_gaps_while_using_measured_heights() {
        let flow = SceneFlowId("flow".to_owned());
        let scene = scene(vec![
            node("a", rect(0.0, 10.0), vec![position(&flow, 0)]),
            node("b", rect(15.0, 10.0), vec![position(&flow, 1)]),
            node("c", rect(30.0, 10.0), vec![position(&flow, 2)]),
        ]);
        let measured = HashMap::from([
            (SceneNodeId("a".to_owned()), 20.0),
            (SceneNodeId("b".to_owned()), 5.0),
        ]);

        let layout = ResolvedLayout::new(&scene, &measured);

        assert_eq!(layout.node_bounds(&scene.nodes[0]), rect(0.0, 20.0));
        assert_eq!(layout.node_bounds(&scene.nodes[1]), rect(25.0, 5.0));
        assert_eq!(layout.node_bounds(&scene.nodes[2]), rect(35.0, 10.0));
        assert_close(layout.bounds.height, 45.0);
    }

    #[test]
    fn keeps_independent_freeform_flows_anchored() {
        let left = SceneFlowId("left".to_owned());
        let right = SceneFlowId("right".to_owned());
        let scene = scene(vec![
            node("left-a", rect(0.0, 10.0), vec![position(&left, 0)]),
            node("left-b", rect(12.0, 10.0), vec![position(&left, 1)]),
            node("right", rect(12.0, 10.0), vec![position(&right, 0)]),
        ]);
        let measured = HashMap::from([(SceneNodeId("left-a".to_owned()), 30.0)]);

        let layout = ResolvedLayout::new(&scene, &measured);

        assert_close(layout.node_bounds(&scene.nodes[1]).y, 32.0);
        assert_close(layout.node_bounds(&scene.nodes[2]).y, 12.0);
    }

    #[test]
    fn nested_flow_growth_expands_its_parent_item_and_moves_later_content() {
        let outer = SceneFlowId("outer".to_owned());
        let inner = SceneFlowId("inner".to_owned());
        let outer_item = position(&outer, 0);
        let scene = scene(vec![
            node("frame", rect(0.0, 20.0), vec![outer_item.clone()]),
            node(
                "inner-a",
                rect(2.0, 5.0),
                vec![outer_item.clone(), position(&inner, 0)],
            ),
            node(
                "inner-b",
                rect(9.0, 5.0),
                vec![outer_item, position(&inner, 1)],
            ),
            node("after", rect(25.0, 10.0), vec![position(&outer, 1)]),
        ]);
        let measured = HashMap::from([(SceneNodeId("inner-a".to_owned()), 30.0)]);

        let layout = ResolvedLayout::new(&scene, &measured);

        assert_close(layout.node_bounds(&scene.nodes[2]).y, 34.0);
        assert_close(layout.node_bounds(&scene.nodes[3]).y, 44.0);
    }

    #[test]
    fn hit_regions_follow_their_resolved_node() {
        let flow = SceneFlowId("flow".to_owned());
        let scene = scene(vec![
            node("a", rect(0.0, 10.0), vec![position(&flow, 0)]),
            node("b", rect(15.0, 10.0), vec![position(&flow, 1)]),
        ]);
        let measured = HashMap::from([(SceneNodeId("a".to_owned()), 25.0)]);
        let layout = ResolvedLayout::new(&scene, &measured);
        let region = HitRegion {
            node_id: SceneNodeId("b".to_owned()),
            source_object_id: ObjectId::new("object"),
            bounds: rect(15.0, 10.0),
            action: HitAction::OpenLink("https://example.test".to_owned()),
        };

        assert_eq!(
            layout.hit_region_bounds(&scene.nodes[1], &region),
            rect(30.0, 10.0)
        );
    }

    #[test]
    fn culling_uses_resolved_bounds_and_recomputation_does_not_drift() {
        let flow = SceneFlowId("flow".to_owned());
        let scene = scene(vec![
            node("a", rect(0.0, 10.0), vec![position(&flow, 0)]),
            node("b", rect(15.0, 10.0), vec![position(&flow, 1)]),
        ]);
        let measured = HashMap::from([(SceneNodeId("a".to_owned()), 100.0)]);
        let first = ResolvedLayout::new(&scene, &measured);
        let second = ResolvedLayout::new(&scene, &measured);
        let viewport = Rect {
            x: 0.0,
            y: 100.0,
            width: 10.0,
            height: 20.0,
        };

        assert_eq!(
            first.node_bounds(&scene.nodes[1]),
            second.node_bounds(&scene.nodes[1])
        );
        assert_eq!(
            first
                .visible_nodes(&scene, viewport, 0.0)
                .map(|node| node.id.0.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    fn scene(nodes: Vec<SceneNode>) -> PageScene {
        let bounds = nodes
            .iter()
            .map(|node| node.bounds)
            .reduce(Rect::union)
            .unwrap_or_default();
        PageScene {
            page_id: PageId::new("page"),
            bounds,
            nodes,
            hit_regions: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn node(id: &str, bounds: Rect, flow_path: Vec<SceneFlowPosition>) -> SceneNode {
        SceneNode {
            id: SceneNodeId(id.to_owned()),
            source_object_id: ObjectId::new("object"),
            bounds,
            flow_path,
            z_index: 0,
            primitive: ScenePrimitive::Fill {
                color: Color {
                    red: 0,
                    green: 0,
                    blue: 0,
                    alpha: 0,
                },
                corner_radius: 0.0,
            },
            accessibility: AccessibilitySemantics {
                role: AccessibilityRole::Decoration,
                label: String::new(),
                description: None,
            },
        }
    }

    fn position(group: &SceneFlowId, order: u32) -> SceneFlowPosition {
        SceneFlowPosition {
            group: group.clone(),
            order,
        }
    }

    fn rect(y: f32, height: f32) -> Rect {
        Rect {
            x: 0.0,
            y,
            width: 10.0,
            height,
        }
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < f32::EPSILON);
    }
}
