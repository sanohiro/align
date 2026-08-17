use std::collections::{HashMap, HashSet};

use crate::{Hash128, IStructDef, IType};

const MAX_CONSTRUCTOR_DEPTH: u16 = 128;
const ABI_CELLS: [u8; 11] = [0, 8, 8, 1, 1, 16, 8, 16, 8, 1, 1];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnedJsonObjectFormat {
    Elf,
    MachO,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedJsonTarget {
    pub triple: String,
    pub object_format: OwnedJsonObjectFormat,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedJsonGraphInterfaceEntry {
    pub type_name: String,
    pub envelope: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
struct Layout {
    size: u32,
    align: u32,
}

#[derive(Clone, Debug)]
enum Node {
    Int { bits: u8, unsigned: bool },
    Bool,
    String,
    Record(usize),
    Option(Box<Node>),
    Array(Box<Node>),
}

#[derive(Clone, Debug)]
struct Graph {
    records: Vec<usize>,
    ordinal: HashMap<usize, u32>,
    layouts: Vec<Layout>,
    offsets: Vec<Vec<u32>>,
    owning: Vec<bool>,
}

fn valid_name(name: &[u8]) -> bool {
    !name.is_empty()
        && name.is_ascii()
        && name.iter().copied().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
        })
}

fn record_index(path: &str, names: &HashMap<&str, usize>) -> Option<usize> {
    names.get(path).copied()
}

fn integer(path: &str) -> Option<(u8, bool)> {
    let (unsigned, digits) = match path.as_bytes().first().copied() {
        Some(b'i') => (false, &path[1..]),
        Some(b'u') => (true, &path[1..]),
        _ => return None,
    };
    let bits = digits.parse().ok()?;
    matches!(bits, 8 | 16 | 32 | 64).then_some((bits, unsigned))
}

fn parse_node(ty: &IType, names: &HashMap<&str, usize>) -> Result<Node, ()> {
    let IType::Named { path, args } = ty else {
        return Err(());
    };
    if args.is_empty() {
        if let Some((bits, unsigned)) = integer(path) {
            return Ok(Node::Int { bits, unsigned });
        }
        if path == "bool" {
            return Ok(Node::Bool);
        }
        if path == "string" {
            return Ok(Node::String);
        }
        return record_index(path, names).map(Node::Record).ok_or(());
    }
    if path == "Option" && args.len() == 1 {
        let payload = parse_node(&args[0], names)?;
        if matches!(payload, Node::Option(_)) {
            return Err(());
        }
        return Ok(Node::Option(Box::new(payload)));
    }
    if path == "array" && args.len() == 1 {
        let element = parse_node(&args[0], names)?;
        if matches!(element, Node::Option(_) | Node::Array(_)) {
            return Err(());
        }
        return Ok(Node::Array(Box::new(element)));
    }
    Err(())
}

fn node_has_string(node: &Node, structs: &[IStructDef], seen: &mut HashSet<usize>) -> bool {
    let mut work = vec![node.clone()];
    while let Some(node) = work.pop() {
        match node {
            Node::String => return true,
            Node::Option(payload) | Node::Array(payload) => work.push(*payload),
            Node::Record(id) if seen.insert(id) => {
                let names = structs
                    .iter()
                    .enumerate()
                    .map(|(index, definition)| (definition.name.as_str(), index))
                    .collect::<HashMap<_, _>>();
                if let Some(definition) = structs.get(id) {
                    for (_, ty) in definition.fields.iter().rev() {
                        if let Ok(child) = parse_node(ty, &names) {
                            work.push(child);
                        }
                    }
                }
            }
            Node::Record(_) | Node::Int { .. } | Node::Bool => {}
        }
    }
    false
}

fn align_up(value: u32, align: u32) -> Option<u32> {
    value
        .checked_add(align.checked_sub(1)?)
        .map(|sum| sum & !(align - 1))
}

fn node_storage_layout(node: &Node, record_layouts: &[Option<Layout>]) -> Option<Layout> {
    match node {
        Node::Int { bits, .. } => {
            let bytes = u32::from(*bits / 8);
            Some(Layout {
                size: bytes,
                align: bytes,
            })
        }
        Node::Bool => Some(Layout { size: 1, align: 1 }),
        Node::String | Node::Array(_) => Some(Layout { size: 16, align: 8 }),
        Node::Record(id) => record_layouts.get(*id).copied().flatten(),
        Node::Option(payload) => {
            let payload = node_storage_layout(payload, record_layouts)?;
            let payload_offset = align_up(1, payload.align)?;
            let size = align_up(payload_offset.checked_add(payload.size)?, payload.align)?;
            Some(Layout {
                size,
                align: payload.align,
            })
        }
    }
}

fn node_owns(node: &Node, owning: &[bool]) -> bool {
    match node {
        Node::String | Node::Array(_) => true,
        Node::Record(id) => owning.get(*id).copied().unwrap_or(false),
        Node::Option(payload) => node_owns(payload, owning),
        Node::Int { .. } | Node::Bool => false,
    }
}

fn next_depth(depth: u16) -> Result<u16, ()> {
    let depth = depth.checked_add(1).ok_or(())?;
    (depth <= MAX_CONSTRUCTOR_DEPTH).then_some(depth).ok_or(())
}

fn node_record_edges(node: &Node, depth: u16, out: &mut Vec<(usize, u16)>) -> Result<(), ()> {
    match node {
        Node::Record(id) => out.push((*id, next_depth(depth)?)),
        Node::Option(payload) | Node::Array(payload) => {
            node_record_edges(payload, next_depth(depth)?, out)?;
        }
        Node::Int { .. } | Node::Bool | Node::String => {}
    }
    Ok(())
}

fn build_graph(structs: &[IStructDef], root: usize) -> Result<Option<Graph>, ()> {
    let names = structs
        .iter()
        .enumerate()
        .map(|(index, definition)| (definition.name.as_str(), index))
        .collect::<HashMap<_, _>>();
    let root_definition = structs.get(root).ok_or(())?;
    if !root_definition.type_params.is_empty() {
        return Ok(None);
    }

    let mut has_string = false;
    for (_, ty) in &root_definition.fields {
        if let Ok(node) = parse_node(ty, &names) {
            has_string |= node_has_string(&node, structs, &mut HashSet::new());
        }
    }
    if !has_string {
        return Ok(None);
    }

    enum Work {
        Enter(usize, u16),
        Exit(usize),
    }
    let mut records = Vec::new();
    let mut discovered = HashSet::new();
    let mut maximum_depth = HashMap::new();
    let mut active = HashSet::new();
    let mut work = vec![Work::Enter(root, 1)];
    while let Some(item) = work.pop() {
        match item {
            Work::Exit(id) => {
                active.remove(&id);
            }
            Work::Enter(id, depth) => {
                if depth > MAX_CONSTRUCTOR_DEPTH || active.contains(&id) {
                    return Err(());
                }
                if maximum_depth.get(&id).is_some_and(|seen| *seen >= depth) {
                    continue;
                }
                maximum_depth.insert(id, depth);
                let first_visit = discovered.insert(id);
                let definition = structs.get(id).ok_or(())?;
                if !definition.type_params.is_empty()
                    || definition.c_repr
                    || definition.align.is_some()
                    || definition.fields.is_empty()
                {
                    return Err(());
                }
                active.insert(id);
                if first_visit {
                    records.push(id);
                }
                work.push(Work::Exit(id));
                let mut edges = Vec::new();
                for (field_name, ty) in &definition.fields {
                    if !valid_name(field_name.as_bytes()) {
                        return Err(());
                    }
                    let node = parse_node(ty, &names)?;
                    node_record_edges(&node, depth, &mut edges)?;
                }
                for edge in edges.into_iter().rev() {
                    work.push(Work::Enter(edge.0, edge.1));
                }
            }
        }
    }

    let ordinal = records
        .iter()
        .enumerate()
        .map(|(ordinal, &id)| Ok((id, u32::try_from(ordinal).map_err(|_| ())?)))
        .collect::<Result<HashMap<_, _>, ()>>()?;

    let mut layouts = vec![None; structs.len()];
    let mut offsets = vec![Vec::new(); structs.len()];
    while records.iter().any(|id| layouts[*id].is_none()) {
        let mut progressed = false;
        for &id in &records {
            if layouts[id].is_some() {
                continue;
            }
            let definition = &structs[id];
            let nodes = definition
                .fields
                .iter()
                .map(|(_, ty)| parse_node(ty, &names))
                .collect::<Result<Vec<_>, _>>()?;
            let Some(field_layouts) = nodes
                .iter()
                .map(|node| node_storage_layout(node, &layouts))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let mut physical = (0..field_layouts.len()).collect::<Vec<_>>();
            physical.sort_by_key(|&index| std::cmp::Reverse(field_layouts[index].align));
            let mut field_offsets = vec![0; field_layouts.len()];
            let mut cursor = 0u32;
            let mut record_align = 1u32;
            for index in physical {
                let layout = field_layouts[index];
                cursor = align_up(cursor, layout.align).ok_or(())?;
                field_offsets[index] = cursor;
                cursor = cursor.checked_add(layout.size).ok_or(())?;
                record_align = record_align.max(layout.align);
            }
            layouts[id] = Some(Layout {
                size: align_up(cursor, record_align).ok_or(())?,
                align: record_align,
            });
            offsets[id] = field_offsets;
            progressed = true;
        }
        if !progressed {
            return Err(());
        }
    }

    let mut owning = vec![false; structs.len()];
    loop {
        let mut changed = false;
        for &id in records.iter().rev() {
            let owns = structs[id].fields.iter().try_fold(false, |owns, (_, ty)| {
                Ok::<_, ()>(owns || node_owns(&parse_node(ty, &names)?, &owning))
            })?;
            if owns && !owning[id] {
                owning[id] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    Ok(Some(Graph {
        records,
        ordinal,
        layouts: layouts
            .into_iter()
            .map(|layout| layout.unwrap_or(Layout { size: 0, align: 0 }))
            .collect(),
        offsets,
        owning,
    }))
}

fn encode_node(bytes: &mut Vec<u8>, node: &Node, graph: &Graph) -> Option<()> {
    let layout = node_storage_layout(
        node,
        &graph.layouts.iter().copied().map(Some).collect::<Vec<_>>(),
    )?;
    let owns = node_owns(node, &graph.owning);
    let (tag, drop_tag) = match node {
        Node::Int { .. } => (0x01, 0),
        Node::Bool => (0x03, 0),
        Node::String => (0x10, 1),
        Node::Record(_) => (0x20, if owns { 2 } else { 0 }),
        Node::Option(_) => (0x21, if owns { 3 } else { 0 }),
        Node::Array(_) => (0x22, 4),
    };
    bytes.push(tag);
    bytes.extend_from_slice(&layout.size.to_le_bytes());
    bytes.extend_from_slice(&layout.align.to_le_bytes());
    bytes.push(u8::from(owns));
    bytes.push(drop_tag);
    match node {
        Node::Int { bits, unsigned } => {
            bytes.push(*bits);
            bytes.push(u8::from(*unsigned));
        }
        Node::Bool | Node::String => {}
        Node::Record(id) => bytes.extend_from_slice(&graph.ordinal.get(id)?.to_le_bytes()),
        Node::Option(payload) => {
            let payload_layout = node_storage_layout(
                payload,
                &graph.layouts.iter().copied().map(Some).collect::<Vec<_>>(),
            )?;
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(&align_up(1, payload_layout.align)?.to_le_bytes());
            encode_node(bytes, payload, graph)?;
        }
        Node::Array(element) => {
            bytes.push(1);
            encode_node(bytes, element, graph)?;
        }
    }
    Some(())
}

fn graph_node_layout(node: &Node, graph: &Graph) -> Option<Layout> {
    node_storage_layout(
        node,
        &graph.layouts.iter().copied().map(Some).collect::<Vec<_>>(),
    )
}

pub fn encode_owned_json_graph_descriptor(
    structs: &[IStructDef],
    root_name: &str,
) -> Option<Vec<u8>> {
    let root = structs
        .iter()
        .position(|definition| definition.name == root_name)?;
    encode_owned_json_graph_descriptor_at(structs, root)
}

fn encode_owned_json_graph_descriptor_at(
    structs: &[IStructDef],
    root: usize,
) -> Option<Vec<u8>> {
    let graph = build_graph(structs, root).ok()??;
    let names = structs
        .iter()
        .enumerate()
        .map(|(index, definition)| (definition.name.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut bytes = vec![2, 0, 1];
    bytes.extend_from_slice(&u32::try_from(graph.records.len()).ok()?.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for &id in &graph.records {
        let definition = structs.get(id)?;
        let layout = graph.layouts[id];
        bytes.extend_from_slice(&layout.size.to_le_bytes());
        bytes.extend_from_slice(&layout.align.to_le_bytes());
        bytes.push(u8::from(graph.owning[id]));
        bytes.push(if graph.owning[id] { 2 } else { 0 });
        bytes.extend_from_slice(&u32::try_from(definition.fields.len()).ok()?.to_le_bytes());
        for (index, (name, ty)) in definition.fields.iter().enumerate() {
            bytes.extend_from_slice(&u32::try_from(name.len()).ok()?.to_le_bytes());
            bytes.extend_from_slice(name.as_bytes());
            encode_node(&mut bytes, &parse_node(ty, &names).ok()?, &graph)?;
            bytes.extend_from_slice(&graph.offsets[id].get(index)?.to_le_bytes());
        }
    }
    Some(bytes)
}

pub fn encode_owned_json_graph_envelope(
    target: &OwnedJsonTarget,
    descriptor: &[u8],
) -> Option<Vec<u8>> {
    if target.triple.is_empty()
        || !target.triple.is_ascii()
        || target.triple.as_bytes().contains(&0)
    {
        return None;
    }
    let mut prefix = vec![2];
    prefix.extend_from_slice(&u32::try_from(target.triple.len()).ok()?.to_le_bytes());
    prefix.extend_from_slice(target.triple.as_bytes());
    prefix.push(match target.object_format {
        OwnedJsonObjectFormat::Elf => 0,
        OwnedJsonObjectFormat::MachO => 1,
    });
    prefix.extend_from_slice(&ABI_CELLS);
    let hash = Hash128::of(&prefix);
    let mut envelope = prefix;
    envelope.extend_from_slice(&hash.lo.to_le_bytes());
    envelope.extend_from_slice(&hash.hi.to_le_bytes());
    envelope.extend_from_slice(&u32::try_from(descriptor.len()).ok()?.to_le_bytes());
    envelope.extend_from_slice(descriptor);
    Some(envelope)
}

pub(crate) fn entries_for_structs(
    structs: &[IStructDef],
    target: &OwnedJsonTarget,
) -> Option<Vec<OwnedJsonGraphInterfaceEntry>> {
    let mut entries = Vec::new();
    for (root, definition) in structs.iter().enumerate() {
        let Ok(Some(_)) = build_graph(structs, root) else {
            continue;
        };
        let descriptor = encode_owned_json_graph_descriptor(structs, &definition.name)?;
        entries.push(OwnedJsonGraphInterfaceEntry {
            type_name: definition.name.clone(),
            envelope: encode_owned_json_graph_envelope(target, &descriptor)?,
        });
    }
    entries.sort_by(|left, right| left.type_name.cmp(&right.type_name));
    Some(entries)
}

pub(crate) fn entries_for_resolved_structs(
    structs: &[IStructDef],
    roots: &[(String, usize)],
    target: &OwnedJsonTarget,
) -> Option<Vec<OwnedJsonGraphInterfaceEntry>> {
    let mut entries = Vec::new();
    for (type_name, root) in roots {
        let Ok(Some(_)) = build_graph(structs, *root) else {
            continue;
        };
        let descriptor = encode_owned_json_graph_descriptor_at(structs, *root)?;
        entries.push(OwnedJsonGraphInterfaceEntry {
            type_name: type_name.clone(),
            envelope: encode_owned_json_graph_envelope(target, &descriptor)?,
        });
    }
    entries.sort_by(|left, right| left.type_name.cmp(&right.type_name));
    Some(entries)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], &'static str> {
        let end = self.pos.checked_add(count).ok_or("length overflow")?;
        let value = self.bytes.get(self.pos..end).ok_or("truncated envelope")?;
        self.pos = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, &'static str> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, &'static str> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().map_err(|_| "truncated u32")?,
        ))
    }

    fn u64(&mut self) -> Result<u64, &'static str> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().map_err(|_| "truncated u64")?,
        ))
    }
}

fn validate_type_node(
    cursor: &mut Cursor<'_>,
    expected: &Node,
    graph: &Graph,
    references: &mut Vec<(u32, u32)>,
    depth: u8,
) -> Result<(), &'static str> {
    if depth > 2 {
        return Err("owned JSON type-node depth");
    }
    let expected_layout = graph_node_layout(expected, graph).ok_or("owned JSON expected layout")?;
    let expected_owns = node_owns(expected, &graph.owning);
    let (expected_tag, expected_drop) = match expected {
        Node::Int { .. } => (0x01, 0),
        Node::Bool => (0x03, 0),
        Node::String => (0x10, 1),
        Node::Record(_) => (0x20, if expected_owns { 2 } else { 0 }),
        Node::Option(_) => (0x21, if expected_owns { 3 } else { 0 }),
        Node::Array(_) => (0x22, 4),
    };
    if cursor.u8().map_err(|_| "owned JSON type tag bound")? != expected_tag {
        return Err("owned JSON type tag");
    }
    if cursor.u32().map_err(|_| "owned JSON type size bound")? != expected_layout.size {
        return Err("owned JSON type size");
    }
    if cursor
        .u32()
        .map_err(|_| "owned JSON type alignment bound")?
        != expected_layout.align
    {
        return Err("owned JSON type alignment");
    }
    if cursor
        .u8()
        .map_err(|_| "owned JSON type allocation bound")?
        != u8::from(expected_owns)
    {
        return Err("owned JSON type allocation");
    }
    if cursor.u8().map_err(|_| "owned JSON type drop bound")? != expected_drop {
        return Err("owned JSON type drop");
    }
    match expected {
        Node::Int { bits, unsigned } => {
            if cursor.u8().map_err(|_| "owned JSON integer bits bound")? != *bits {
                return Err("owned JSON integer bits");
            }
            if cursor.u8().map_err(|_| "owned JSON integer sign bound")? != u8::from(*unsigned) {
                return Err("owned JSON integer sign");
            }
        }
        Node::Bool | Node::String => {}
        Node::Record(id) => {
            let actual = cursor
                .u32()
                .map_err(|_| "owned JSON record reference bound")?;
            let expected = *graph
                .ordinal
                .get(id)
                .ok_or("owned JSON expected record reference")?;
            references.push((actual, expected));
        }
        Node::Option(payload) => {
            if cursor
                .u32()
                .map_err(|_| "owned JSON option tag offset bound")?
                != 0
            {
                return Err("owned JSON option tag offset");
            }
            let payload_layout =
                graph_node_layout(payload, graph).ok_or("owned JSON option payload layout")?;
            let payload_offset =
                align_up(1, payload_layout.align).ok_or("owned JSON option payload offset")?;
            if cursor
                .u32()
                .map_err(|_| "owned JSON option payload offset bound")?
                != payload_offset
            {
                return Err("owned JSON option payload offset");
            }
            validate_type_node(cursor, payload, graph, references, depth + 1)?;
        }
        Node::Array(element) => {
            if cursor.u8().map_err(|_| "owned JSON array plan bound")? != 1 {
                return Err("owned JSON array plan version");
            }
            validate_type_node(cursor, element, graph, references, depth + 1)?;
        }
    }
    Ok(())
}

fn validate_descriptor(
    structs: &[IStructDef],
    root_name: &str,
    descriptor: &[u8],
) -> Result<(), &'static str> {
    let root = structs
        .iter()
        .position(|definition| definition.name == root_name)
        .ok_or("owned JSON graph root")?;
    let graph = build_graph(structs, root)
        .map_err(|_| "owned JSON semantic graph")?
        .ok_or("owned JSON semantic graph")?;
    let names = structs
        .iter()
        .enumerate()
        .map(|(index, definition)| (definition.name.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut cursor = Cursor {
        bytes: descriptor,
        pos: 0,
    };
    if cursor
        .u8()
        .map_err(|_| "owned JSON descriptor version bound")?
        != 2
    {
        return Err("owned JSON descriptor version");
    }
    if cursor.u8().map_err(|_| "owned JSON layout mode bound")? != 0 {
        return Err("owned JSON layout mode");
    }
    if cursor
        .u8()
        .map_err(|_| "owned JSON layout algorithm bound")?
        != 1
    {
        return Err("owned JSON layout algorithm");
    }
    let record_count = cursor.u32().map_err(|_| "owned JSON record count bound")?;
    if record_count == 0 {
        return Err("owned JSON empty record graph");
    }
    if usize::try_from(record_count).ok() != Some(graph.records.len()) {
        return Err("owned JSON record count");
    }
    if cursor.u32().map_err(|_| "owned JSON root ordinal bound")? != 0 {
        return Err("owned JSON root ordinal");
    }

    let mut references = Vec::new();
    for &id in &graph.records {
        let definition = structs.get(id).ok_or("owned JSON record definition")?;
        let layout = graph.layouts[id];
        if cursor.u32().map_err(|_| "owned JSON record size bound")? != layout.size {
            return Err("owned JSON record size");
        }
        if cursor
            .u32()
            .map_err(|_| "owned JSON record alignment bound")?
            != layout.align
        {
            return Err("owned JSON record alignment");
        }
        if cursor
            .u8()
            .map_err(|_| "owned JSON record allocation bound")?
            != u8::from(graph.owning[id])
        {
            return Err("owned JSON record allocation");
        }
        if cursor.u8().map_err(|_| "owned JSON record drop bound")?
            != if graph.owning[id] { 2 } else { 0 }
        {
            return Err("owned JSON record drop");
        }
        let field_count = cursor.u32().map_err(|_| "owned JSON field count bound")?;
        if field_count == 0 {
            return Err("owned JSON empty record");
        }
        if usize::try_from(field_count).ok() != Some(definition.fields.len()) {
            return Err("owned JSON field count");
        }
        let mut seen = HashSet::new();
        for (index, (expected_name, expected_ty)) in definition.fields.iter().enumerate() {
            let name_len = usize::try_from(
                cursor
                    .u32()
                    .map_err(|_| "owned JSON field name length bound")?,
            )
            .map_err(|_| "owned JSON field name length")?;
            let name = cursor
                .take(name_len)
                .map_err(|_| "owned JSON field name bound")?;
            if !valid_name(name) {
                return Err("owned JSON field name grammar");
            }
            if !seen.insert(name) {
                return Err("owned JSON duplicate field name");
            }
            if name != expected_name.as_bytes() {
                return Err("owned JSON field name/order");
            }
            let expected_node =
                parse_node(expected_ty, &names).map_err(|_| "owned JSON expected field type")?;
            validate_type_node(&mut cursor, &expected_node, &graph, &mut references, 0)?;
            if cursor
                .u32()
                .map_err(|_| "owned JSON physical offset bound")?
                != *graph.offsets[id]
                    .get(index)
                    .ok_or("owned JSON expected physical offset")?
            {
                return Err("owned JSON physical offset");
            }
        }
    }
    if cursor.pos != descriptor.len() {
        return Err("owned JSON descriptor trailing bytes");
    }
    for (actual, expected) in references {
        if usize::try_from(actual)
            .ok()
            .filter(|ordinal| *ordinal < graph.records.len())
            .is_none()
        {
            return Err("owned JSON record reference bounds");
        }
        if actual != expected {
            return Err("owned JSON record reference order");
        }
    }
    Ok(())
}

fn descriptor_node(
    cursor: &mut Cursor<'_>,
    record_names: &[String],
    depth: u8,
) -> Result<IType, &'static str> {
    if depth > 2 {
        return Err("owned JSON type-node depth");
    }
    let tag = cursor.u8()?;
    cursor.u32()?;
    cursor.u32()?;
    cursor.u8()?;
    cursor.u8()?;
    let named = |path: String| IType::Named { path, args: Vec::new() };
    match tag {
        0x01 => {
            let bits = cursor.u8()?;
            let unsigned = cursor.u8()?;
            Ok(named(format!("{}{}", if unsigned == 0 { 'i' } else { 'u' }, bits)))
        }
        0x03 => Ok(named("bool".to_string())),
        0x10 => Ok(named("string".to_string())),
        0x20 => {
            let ordinal = usize::try_from(cursor.u32()?)
                .map_err(|_| "owned JSON record reference bounds")?;
            let name = record_names.get(ordinal).ok_or("owned JSON record reference bounds")?;
            Ok(named(name.clone()))
        }
        0x21 => {
            cursor.u32()?;
            cursor.u32()?;
            Ok(IType::Named {
                path: "Option".to_string(),
                args: vec![descriptor_node(cursor, record_names, depth + 1)?],
            })
        }
        0x22 => {
            cursor.u8()?;
            Ok(IType::Named {
                path: "array".to_string(),
                args: vec![descriptor_node(cursor, record_names, depth + 1)?],
            })
        }
        _ => Err("owned JSON type tag"),
    }
}

fn descriptor_structs(
    root_name: &str,
    descriptor: &[u8],
) -> Result<Vec<IStructDef>, &'static str> {
    let mut cursor = Cursor { bytes: descriptor, pos: 0 };
    cursor.u8()?;
    cursor.u8()?;
    cursor.u8()?;
    let record_count =
        usize::try_from(cursor.u32()?).map_err(|_| "owned JSON record count")?;
    if record_count == 0 || record_count > descriptor.len() / 14 {
        return Err("owned JSON record count");
    }
    cursor.u32()?;
    let record_names = (0..record_count)
        .map(|ordinal| {
            if ordinal == 0 {
                root_name.to_string()
            } else {
                format!("__owned_json_record_{ordinal}")
            }
        })
        .collect::<Vec<_>>();
    let mut structs = Vec::new();
    for name in &record_names {
        cursor.u32()?;
        cursor.u32()?;
        cursor.u8()?;
        cursor.u8()?;
        let field_count =
            usize::try_from(cursor.u32()?).map_err(|_| "owned JSON field count")?;
        if field_count == 0 || field_count > descriptor.len() / 5 {
            return Err("owned JSON field count");
        }
        let mut fields = Vec::new();
        for _ in 0..field_count {
            let name_len = usize::try_from(cursor.u32()?)
                .map_err(|_| "owned JSON field name length")?;
            let field_name = std::str::from_utf8(cursor.take(name_len)?)
                .map_err(|_| "owned JSON field name grammar")?
                .to_string();
            let ty = descriptor_node(&mut cursor, &record_names, 0)?;
            cursor.u32()?;
            fields.push((field_name, ty));
        }
        structs.push(IStructDef {
            name: name.clone(),
            type_params: Vec::new(),
            fields,
            align: None,
            c_repr: false,
            generic_body: None,
        });
    }
    if cursor.pos != descriptor.len() {
        return Err("owned JSON descriptor trailing bytes");
    }
    Ok(structs)
}

fn root_shape_matches(expected: &IStructDef, actual: &IStructDef) -> bool {
    fn ty_matches(expected: &IType, actual: &IType) -> bool {
        let (
            IType::Named { path: expected_path, args: expected_args },
            IType::Named { path: actual_path, args: actual_args },
        ) = (expected, actual)
        else {
            return false;
        };
        if matches!(expected_path.as_str(), "Option" | "array") {
            return expected_path == actual_path
                && expected_args.len() == 1
                && actual_args.len() == 1
                && ty_matches(&expected_args[0], &actual_args[0]);
        }
        if integer(expected_path).is_some()
            || matches!(expected_path.as_str(), "bool" | "string")
        {
            return expected_path == actual_path
                && expected_args.is_empty()
                && actual_args.is_empty();
        }
        if matches!(
            expected_path.as_str(),
            "str" | "char" | "f32" | "f64" | "Result" | "slice" | "soa" | "raw" | "()"
        ) {
            return false;
        }
        actual_path.starts_with("__owned_json_record_") && actual_args.is_empty()
    }

    expected.fields.len() == actual.fields.len()
        && expected.fields.iter().zip(&actual.fields).all(
            |((expected_name, expected_ty), (actual_name, actual_ty))| {
                expected_name == actual_name && ty_matches(expected_ty, actual_ty)
            },
        )
}

pub(crate) fn validate_entries(
    structs: &[IStructDef],
    entries: &[OwnedJsonGraphInterfaceEntry],
    current: Option<&OwnedJsonTarget>,
) -> Result<(), &'static str> {
    let expected = entries_for_structs(
        structs,
        current.unwrap_or(&OwnedJsonTarget {
            triple: "x86_64-pc-linux-gnu".to_string(),
            object_format: OwnedJsonObjectFormat::Elf,
        }),
    )
    .ok_or("owned JSON graph")?;
    let expected_names = expected
        .iter()
        .map(|entry| entry.type_name.as_str())
        .collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut previous = None;
    for entry in entries {
        if !valid_name(entry.type_name.as_bytes()) {
            return Err("owned JSON graph entry name grammar");
        }
        if previous.is_some_and(|name| name >= entry.type_name.as_str()) {
            return Err("owned JSON graph name/order");
        }
        previous = Some(entry.type_name.as_str());
        if !seen.insert(entry.type_name.as_str()) {
            return Err("owned JSON graph name/order");
        }
        let root = structs
            .iter()
            .position(|definition| definition.name == entry.type_name)
            .ok_or("owned JSON graph root")?;
        if !structs[root].type_params.is_empty() {
            return Err("owned JSON graph root");
        }
        let mut cursor = Cursor {
            bytes: &entry.envelope,
            pos: 0,
        };
        if cursor.u8()? != 2 {
            return Err("owned JSON envelope version");
        }
        let triple_len = usize::try_from(cursor.u32()?).map_err(|_| "target triple length")?;
        if triple_len == 0 {
            return Err("empty target triple");
        }
        let triple = cursor.take(triple_len)?;
        if !triple.is_ascii() || triple.contains(&0) {
            return Err("invalid target triple");
        }
        let object = cursor.u8()?;
        if object > 1 {
            return Err("object format");
        }
        if cursor.take(ABI_CELLS.len())? != ABI_CELLS {
            return Err("target ABI cells");
        }
        let prefix_end = cursor.pos;
        let stored = Hash128 {
            lo: cursor.u64()?,
            hi: cursor.u64()?,
        };
        if stored != Hash128::of(&entry.envelope[..prefix_end]) {
            return Err("target ABI hash");
        }
        if let Some(target) = current {
            if triple != target.triple.as_bytes() {
                return Err("target triple mismatch");
            }
            let expected_object = match target.object_format {
                OwnedJsonObjectFormat::Elf => 0,
                OwnedJsonObjectFormat::MachO => 1,
            };
            if object != expected_object {
                return Err("object format mismatch");
            }
        }
        let descriptor_len =
            usize::try_from(cursor.u32()?).map_err(|_| "owned JSON descriptor length")?;
        let descriptor = cursor
            .take(descriptor_len)
            .map_err(|_| "owned JSON descriptor length/bound")?;
        if cursor.pos != entry.envelope.len() {
            return Err("owned JSON envelope trailing bytes");
        }
        match build_graph(structs, root) {
            Ok(Some(_)) => validate_descriptor(structs, &entry.type_name, descriptor)?,
            Ok(None) | Err(()) => {
                let descriptor_definitions = descriptor_structs(&entry.type_name, descriptor)?;
                if !root_shape_matches(&structs[root], &descriptor_definitions[0]) {
                    return Err("owned JSON root field graph");
                }
                validate_descriptor(&descriptor_definitions, &entry.type_name, descriptor)?;
            }
        }
    }
    if !expected_names.is_subset(&seen) {
        return Err("owned JSON graph cardinality");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ty(path: &str) -> IType {
        IType::Named {
            path: path.to_string(),
            args: Vec::new(),
        }
    }

    fn app(path: &str, argument: IType) -> IType {
        IType::Named {
            path: path.to_string(),
            args: vec![argument],
        }
    }

    fn definition(name: &str, fields: Vec<(&str, IType)>) -> IStructDef {
        IStructDef {
            name: name.to_string(),
            type_params: Vec::new(),
            fields: fields
                .into_iter()
                .map(|(name, ty)| (name.to_string(), ty))
                .collect(),
            align: None,
            c_repr: false,
            generic_body: None,
        }
    }

    fn graph() -> Vec<IStructDef> {
        vec![
            definition(
                "OwnedEnvelope",
                vec![
                    ("version", ty("u16")),
                    ("child", ty("OwnedLeaf")),
                    ("note", app("Option", ty("string"))),
                    ("items", app("array", ty("OwnedLeaf"))),
                ],
            ),
            definition(
                "OwnedLeaf",
                vec![("ok", ty("bool")), ("text", ty("string"))],
            ),
        ]
    }

    fn hex(text: &str) -> Vec<u8> {
        text.split_ascii_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).expect("hex byte"))
            .collect()
    }

    #[test]
    fn recursive_descriptor_and_envelope_match_normative_goldens() {
        let descriptor = encode_owned_json_graph_descriptor(&graph(), "OwnedEnvelope").unwrap();
        assert_eq!(descriptor.len(), 221);
        assert_eq!(
            descriptor,
            hex("02 00 01 02 00 00 00 00 00 00 00 48 00 00 00 08
             00 00 00 01 02 04 00 00 00 07 00 00 00 76 65 72
             73 69 6f 6e 01 02 00 00 00 02 00 00 00 00 00 10
             01 40 00 00 00 05 00 00 00 63 68 69 6c 64 20 18
             00 00 00 08 00 00 00 01 02 01 00 00 00 00 00 00 00
             04 00 00 00 6e 6f 74 65 21 18 00 00 00 08 00 00
             00 01 03 00 00 00 00 08 00 00 00 10 10 00 00 00
             08 00 00 00 01 01 18 00 00 00 05 00 00 00 69 74
             65 6d 73 22 10 00 00 00 08 00 00 00 01 04 01 20
             18 00 00 00 08 00 00 00 01 02 01 00 00 00 30 00
             00 00 18 00 00 00 08 00 00 00 01 02 02 00 00 00
             02 00 00 00 6f 6b 03 01 00 00 00 01 00 00 00 00
             00 10 00 00 00 04 00 00 00 74 65 78 74 10 10 00
             00 00 08 00 00 00 01 01 00 00 00 00")
        );
        let target = OwnedJsonTarget {
            triple: "x86_64-pc-linux-gnu".to_string(),
            object_format: OwnedJsonObjectFormat::Elf,
        };
        let envelope = encode_owned_json_graph_envelope(&target, &descriptor).unwrap();
        assert_eq!(
            &envelope[..36],
            &hex("02 13 00 00 00 78 38 36 5f 36 34 2d 70 63 2d 6c
             69 6e 75 78 2d 67 6e 75 00 00 08 08 01 01 10 08
             10 08 01 01")
        );
        assert_eq!(
            &envelope[36..52],
            &hex("17 73 45 bb fc 42 7d 00 dc a3 b5 9c f9 79 f1 c8")
        );
    }

    #[test]
    fn shared_dag_depth_is_checked_on_every_path() {
        let mut structs = vec![
            definition("Leaf", vec![("text", ty("string"))]),
            definition("Shared", vec![("leaf", ty("Leaf"))]),
        ];
        for id in 0..126 {
            let next = if id == 125 {
                "Shared".to_string()
            } else {
                format!("Chain{}", id + 1)
            };
            structs.push(definition(
                &format!("Chain{id}"),
                vec![("next", ty(&next))],
            ));
        }
        let root = structs.len();
        structs.push(definition(
            "Root",
            vec![("shallow", ty("Shared")), ("deep", ty("Chain0"))],
        ));
        assert!(
            build_graph(&structs, root).is_err(),
            "the deeper occurrence of Shared carries Leaf past depth 128"
        );
    }

    #[test]
    fn target_and_descriptor_mutations_fail_closed() {
        let structs = graph();
        let target = OwnedJsonTarget {
            triple: "x86_64-pc-linux-gnu".to_string(),
            object_format: OwnedJsonObjectFormat::Elf,
        };
        let mut entries = entries_for_structs(&structs, &target).unwrap();
        assert_eq!(entries.len(), 2);
        entries[0].envelope[36] ^= 1;
        assert_eq!(
            validate_entries(&structs, &entries, Some(&target)),
            Err("target ABI hash")
        );

        let mut entries = entries_for_structs(&structs, &target).unwrap();
        let last = entries[0].envelope.len() - 1;
        entries[0].envelope[last] ^= 1;
        assert_eq!(
            validate_entries(&structs, &entries, Some(&target)),
            Err("owned JSON physical offset")
        );

        let baseline = entries_for_structs(&structs, &target).unwrap();
        let descriptor_start = 56;
        for offset in descriptor_start..baseline[0].envelope.len() {
            let mut mutated = baseline.clone();
            mutated[0].envelope[offset] ^= 1;
            assert!(
                validate_entries(&structs, &mutated, Some(&target)).is_err(),
                "descriptor byte {offset} must be validated"
            );
        }
        for length in 0..baseline[0].envelope.len() {
            let mut truncated = baseline.clone();
            truncated[0].envelope.truncate(length);
            assert!(
                validate_entries(&structs, &truncated, Some(&target)).is_err(),
                "envelope truncation at {length} must fail closed"
            );
        }
    }
}
