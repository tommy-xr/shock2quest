use std::{
    collections::{HashMap, HashSet},
    io::{self, SeekFrom},
    rc::Rc,
};

use shipyard::{EntityId, World};
use tracing::trace;

use crate::{
    properties::{
        LinkDefinition, LinkDefinitionWithData, PropSymName, PropTemplateId, Property,
        PropertyDefinition, TemplateLinks, ToTemplateLinkInfo,
    },
    ss2_chunk_file_reader::ChunkFileTableOfContents,
    ss2_common::{read_bytes, read_i32, read_u16, read_u32},
    util::merge_maps,
    Gamesys,
};

#[derive(Debug, Clone)]
pub struct UnparsedProperty {
    pub entity_id: i32,
    pub byte_len: u32,
}

#[derive(Debug, Clone)]
pub struct UnparsedLinkData {
    pub link_id: i32,
    pub byte_len: u32,
}

#[derive(Debug)]
pub struct SystemShock2EntityInfo {
    pub entity_to_properties: HashMap<i32, Vec<Rc<Box<dyn Property>>>>,
    pub template_to_links: HashMap<i32, TemplateLinks>,
    pub unparsed_properties: HashMap<String, Vec<UnparsedProperty>>,
    pub unparsed_links: HashMap<String, Vec<Link>>,
    pub unparsed_link_data: HashMap<String, Vec<UnparsedLinkData>>,

    // TODO: Create dictionary for these?
    // Or create a HashMap entity_to_links?
    pub link_playerfactories: Vec<Link>,
    link_metaprops: Vec<Link>,

    // For each template id, store a list of ancestor template ids
    hierarchy: HashMap<i32, Vec<i32>>,
}

impl SystemShock2EntityInfo {
    /// Create an empty SystemShock2EntityInfo for debug or testing purposes
    /// This will be properly initialized when merged with gamesys data
    pub fn empty() -> SystemShock2EntityInfo {
        SystemShock2EntityInfo {
            entity_to_properties: HashMap::new(),
            template_to_links: HashMap::new(),
            unparsed_properties: HashMap::new(),
            unparsed_links: HashMap::new(),
            unparsed_link_data: HashMap::new(),
            link_playerfactories: Vec::new(),
            link_metaprops: Vec::new(),
            hierarchy: HashMap::new(),
        }
    }

    pub fn initialize_world_with_entities(
        &self,
        world: &mut World,
        name_map_override: HashMap<i32, String>,
        fn_should_initialize: impl Fn(i32) -> bool,
    ) -> HashMap<i32, EntityId> {
        let hierarchy = get_hierarchy(self);
        let mut template_to_entity_id = HashMap::new();
        let mut entity_to_template_id = HashMap::new();
        let mut name_to_template_id = HashMap::new();
        for (id, _props) in &self.entity_to_properties {
            if !fn_should_initialize(*id) {
                tracing::debug!("Skipping entity initialization: {}", id);
                continue;
            }
            tracing::debug!("Initializing entity: {}", id);

            // Create the entity
            let entity = world.add_entity(());
            template_to_entity_id.insert(*id, entity);
            entity_to_template_id.insert(entity, *id);
            let mut ancestors = get_ancestors(hierarchy, id);
            ancestors.push(*id);

            world.add_component(entity, PropTemplateId { template_id: *id });

            for parent_id in ancestors {
                let maybe_parent_props = self.entity_to_properties.get(&parent_id);

                match maybe_parent_props {
                    None => {}
                    Some(props) => {
                        for prop in props {
                            prop.initialize(world, entity)
                        }
                    }
                }

                // Add, override name if specified in name map
                if let Some(name) = name_map_override.get(&parent_id) {
                    let lowercase_name = name.to_ascii_lowercase();
                    name_to_template_id.insert(lowercase_name, id);
                    world.add_component(entity, PropSymName(name.to_owned()))
                }
            }
        }
        template_to_entity_id
    }
}

///
/// merge_with_gamesys
///
/// Helper function to augment an entity info with the gamesys - bringing in ancestor / inherited entities
/// TODO: Maybe gamesys should just be passed when loading the mission, so this occurs 'for free'?
pub fn merge_with_gamesys(
    map_info: &SystemShock2EntityInfo,
    gamesys: &Gamesys,
) -> SystemShock2EntityInfo {
    let mut link_metaprops = map_info.link_metaprops.clone();

    let gamesys_entity_info = &gamesys.entity_info;
    let l2 = gamesys_entity_info.link_metaprops.clone();

    let mut existing_links = HashSet::new();

    for link in &link_metaprops {
        existing_links.insert(link.src);
    }

    // Filter out lower-priority links
    let mut new_links: Vec<Link> = l2
        .iter()
        .filter(|link| !existing_links.contains(&link.src))
        .cloned()
        .collect();
    link_metaprops.append(&mut new_links);

    let mut entity_to_properties = HashMap::new();

    for (id, props) in &map_info.entity_to_properties {
        //let mut vec = Vec::new();
        for _prop in props {}
        entity_to_properties.insert(*id, props.clone());
    }

    for (id, props) in &gamesys_entity_info.entity_to_properties {
        if entity_to_properties.contains_key(id) {
            panic!("duplicate entity {id}")
        }
        //let mut vec = Vec::new();
        for _prop in props {}
        entity_to_properties.insert(*id, props.clone());
    }

    let template_to_links = merge_maps(
        &gamesys_entity_info.template_to_links,
        &map_info.template_to_links,
        |a: &TemplateLinks, b: &TemplateLinks, _k: &i32| TemplateLinks::merge(a, b),
        TemplateLinks::empty(),
    );
    let hierarchy = calculate_hierarchy(&link_metaprops);

    let mut unparsed_properties = map_info.unparsed_properties.clone();
    for (chunk, entries) in &gamesys_entity_info.unparsed_properties {
        unparsed_properties
            .entry(chunk.clone())
            .or_insert_with(Vec::new)
            .extend(entries.clone());
    }

    let mut unparsed_links = map_info.unparsed_links.clone();
    for (chunk, entries) in &gamesys_entity_info.unparsed_links {
        unparsed_links
            .entry(chunk.clone())
            .or_insert_with(Vec::new)
            .extend(entries.clone());
    }

    let mut unparsed_link_data = map_info.unparsed_link_data.clone();
    for (chunk, entries) in &gamesys_entity_info.unparsed_link_data {
        unparsed_link_data
            .entry(chunk.clone())
            .or_insert_with(Vec::new)
            .extend(entries.clone());
    }

    SystemShock2EntityInfo {
        entity_to_properties,
        unparsed_properties,
        unparsed_links,
        unparsed_link_data,
        link_metaprops,
        // TODO: Does this need to be merged?
        link_playerfactories: map_info.link_playerfactories.clone(),
        // TODO: Does this need to be merged?
        template_to_links,
        hierarchy,
    }
}

/* Create a map from entity template id -> parent template ids */
pub fn get_hierarchy(entity_info: &SystemShock2EntityInfo) -> &HashMap<i32, Vec<i32>> {
    &entity_info.hierarchy
}

pub fn calculate_hierarchy(link_metaprops: &Vec<Link>) -> HashMap<i32, Vec<i32>> {
    let mut ret = HashMap::new();
    let metaprops = link_metaprops;
    for link in metaprops {
        let child = link.src;
        let parent = link.dest;

        ret.entry(child).or_insert_with(Vec::new);

        let parents = ret.get_mut(&child).unwrap();
        parents.push(parent);
    }
    ret
}

pub fn get_ancestors(hierarchy: &HashMap<i32, Vec<i32>>, id: &i32) -> Vec<i32> {
    let parents = hierarchy.get(id);

    let mut out = Vec::new();
    let mut visited = HashSet::new();

    traverse_and_add_parents(hierarchy, &mut visited, parents, &mut out);
    out.dedup();
    out.reverse();
    out
}

fn traverse_and_add_parents(
    hierarchy: &HashMap<i32, Vec<i32>>,
    visited: &mut HashSet<i32>,
    maybe_parents: Option<&Vec<i32>>,
    out: &mut Vec<i32>,
) {
    // First, add current parents to array
    match maybe_parents {
        // No parents, nothing to do
        None => {}
        Some(parents) => {
            for p in parents {
                if !visited.contains(p) {
                    out.push(*p);
                    visited.insert(*p);
                }
            }

            for p in parents {
                let grandparent = hierarchy.get(p);
                traverse_and_add_parents(hierarchy, visited, grandparent, out);
            }
        }
    }
}

pub fn new<R: io::Read + io::Seek>(
    toc: &ChunkFileTableOfContents,
    links: &Vec<Box<dyn LinkDefinition>>,
    links_with_data: &Vec<Box<dyn LinkDefinitionWithData>>,
    properties: &Vec<Box<dyn PropertyDefinition<R>>>,
    reader: &mut R,
) -> SystemShock2EntityInfo {
    let entity_to_properties = read_all_properties(toc, properties, reader);

    let link_metaprops = read_link("L$MetaProp", reader, toc);

    let link_playerfactories = read_link("L$PlayerFac", reader, toc);
    tracing::debug!("Player factory links: {link_playerfactories:#?}");

    let mut template_to_links = read_all_links(toc, links, reader);
    read_all_data_links(toc, &mut template_to_links, links_with_data, reader);

    let hierarchy = calculate_hierarchy(&link_metaprops);

    let known_property_chunks: HashSet<String> =
        properties.iter().map(|prop| prop.name()).collect();

    let mut known_link_chunks: HashSet<String> = links.iter().map(|link| link.name()).collect();
    known_link_chunks.extend(links_with_data.iter().map(|link| link.link_chunk_name()));
    known_link_chunks.insert("L$MetaProp".to_string());
    known_link_chunks.insert("L$PlayerFac".to_string());

    let known_link_data_chunks: HashSet<String> = links_with_data
        .iter()
        .map(|link| link.link_data_chunk_name())
        .collect();

    let mut unparsed_properties: HashMap<String, Vec<UnparsedProperty>> = HashMap::new();
    let mut unparsed_links: HashMap<String, Vec<Link>> = HashMap::new();
    let mut unparsed_link_data: HashMap<String, Vec<UnparsedLinkData>> = HashMap::new();

    let chunk_names: Vec<String> = toc.chunk_names().cloned().collect();
    for chunk_name in chunk_names {
        if chunk_name.starts_with("P$") && !known_property_chunks.contains(chunk_name.as_str()) {
            let entries = read_unparsed_property_chunk(toc, &chunk_name, reader);
            unparsed_properties.insert(chunk_name, entries);
        } else if chunk_name.starts_with("L$") && !known_link_chunks.contains(chunk_name.as_str()) {
            let links_for_chunk = read_link(&chunk_name, reader, toc);
            unparsed_links.insert(chunk_name, links_for_chunk);
        } else if chunk_name.starts_with("LD$")
            && !known_link_data_chunks.contains(chunk_name.as_str())
        {
            let entries = read_unparsed_link_data_chunk(toc, &chunk_name, reader);
            unparsed_link_data.insert(chunk_name, entries);
        }
    }

    SystemShock2EntityInfo {
        entity_to_properties,
        template_to_links,
        unparsed_properties,
        unparsed_links,
        unparsed_link_data,
        link_playerfactories,
        link_metaprops,
        hierarchy,
    }
}
#[derive(Debug, Clone)]
pub struct Link {
    pub id: i32,
    pub src: i32,
    pub dest: i32,
    pub flavor: u16,
    pub name: String,
}

pub fn read_link<T: io::Read + io::Seek>(
    link_chunk_name: &str,
    reader: &mut T,
    toc: &ChunkFileTableOfContents,
) -> Vec<Link> {
    let mut ret = vec![];

    if let Some(chunk_pos) = toc.get_chunk(link_chunk_name.to_owned()) {
        trace!("reading chunk: {}", link_chunk_name);
        reader.seek(SeekFrom::Start(chunk_pos.offset)).unwrap();

        let end_pos = chunk_pos.offset + chunk_pos.length;
        while reader.stream_position().unwrap() < end_pos {
            let id = read_i32(reader);
            let src = read_i32(reader);
            let dest = read_i32(reader);
            let flavor = read_u16(reader);

            let link = Link {
                id,
                src,
                dest,
                flavor,
                name: link_chunk_name.to_owned(),
            };

            ret.push(link);
        }
    }

    ret
}

fn read_all_data_links<R1: io::Read + io::Seek>(
    toc: &ChunkFileTableOfContents,
    ent_to_links: &mut HashMap<i32, TemplateLinks>,
    links: &Vec<Box<dyn LinkDefinitionWithData>>,
    ref_reader: &mut R1,
) {
    for link in links {
        let chunk_name = link.link_chunk_name();
        let data_chunk_name = link.link_data_chunk_name();

        let link_infos = read_link(&chunk_name, ref_reader, toc);
        let link_data = read_link_data(&data_chunk_name, ref_reader, toc, link_infos.len() as u32);

        for link_info in link_infos {
            let to_link = ToTemplateLinkInfo {
                id: link_info.id,
                dest_template_id: link_info.dest,
                flavor: link_info.flavor,
            };

            if let Some(data) = link_data.get(&link_info.id) {
                let component_link = link.convert(data.clone(), data.len() as u32, to_link);

                ent_to_links
                    .entry(link_info.src)
                    .or_insert_with(|| TemplateLinks { to_links: vec![] });

                let links = ent_to_links.get_mut(&link_info.src).unwrap();
                links.to_links.push(component_link);
            }
        }
    }
}

pub fn read_link_data<T: io::Read + io::Seek>(
    link_data_chunk_name: &str,
    reader: &mut T,
    toc: &ChunkFileTableOfContents,
    count: u32,
) -> HashMap<i32, Vec<u8>> {
    let mut data = HashMap::new();

    if count > 0 {
        if let Some(chunk_pos) = toc.get_chunk(link_data_chunk_name.to_owned()) {
            trace!(
                "reading chunk: {} length: {}, count: {}",
                link_data_chunk_name,
                chunk_pos.length,
                count
            );
            reader.seek(SeekFrom::Start(chunk_pos.offset)).unwrap();

            // Figure out the size for each individual link entry
            let _link_data_size = (chunk_pos.length / count as u64) - 4;

            let end_pos = chunk_pos.offset + chunk_pos.length;
            let data_len = read_u32(reader) as u64;
            // trace!("data_len: {} link_data_size: {}", data_len, link_data_size);
            // assert!(data_len == link_data_size);
            while reader.stream_position().unwrap() < end_pos {
                let id = read_i32(reader);
                // println!("link id: {}", id);
                let bytes = read_bytes(reader, data_len as usize);
                data.insert(id, bytes);
            }
            // panic!("link data size: {}", link_data_size);
        }
    }

    data
}

fn read_all_links<R: io::Read + io::Seek>(
    toc: &ChunkFileTableOfContents,
    links: &Vec<Box<dyn LinkDefinition>>,
    ref_reader: &mut R,
) -> HashMap<i32, TemplateLinks> {
    let mut ent_to_links: HashMap<i32, TemplateLinks> = HashMap::new();

    for link in links {
        let name = link.name();

        let link_infos = read_link(&name.to_owned(), ref_reader, toc);

        for link_info in link_infos {
            let to_link = ToTemplateLinkInfo {
                id: link_info.id,
                dest_template_id: link_info.dest,
                flavor: link_info.flavor,
            };

            let component_link = link.convert(to_link);

            ent_to_links
                .entry(link_info.src)
                .or_insert_with(|| TemplateLinks { to_links: vec![] });

            let links = ent_to_links.get_mut(&link_info.src).unwrap();
            links.to_links.push(component_link);
        }
    }

    ent_to_links
}

fn read_all_properties<R: io::Read + io::Seek>(
    toc: &ChunkFileTableOfContents,
    properties: &Vec<Box<dyn PropertyDefinition<R>>>,
    ref_reader: &mut R,
) -> HashMap<i32, Vec<Rc<Box<dyn Property>>>> {
    let mut ent_to_props = HashMap::new();
    for prop in properties {
        let name = prop.name();
        trace!("Reading prop: {}", &name);

        if !toc.has_chunk(name.to_owned()) {
            continue;
        } else {
            let chunk = toc.get_chunk(name.to_owned()).unwrap();
            trace!("Chunk: {chunk:?}");
            ref_reader.seek(SeekFrom::Start(chunk.offset)).unwrap();

            let end_pos = chunk.offset + chunk.length;
            while ref_reader.stream_position().unwrap() < end_pos {
                let obj_id = read_i32(ref_reader);
                let prop_len = read_u32(ref_reader);

                let expected_pos = ref_reader.stream_position().unwrap() + prop_len as u64;
                let prop = prop.read(ref_reader, prop_len);

                ent_to_props.entry(obj_id).or_insert_with(Vec::new);

                let props = ent_to_props.get_mut(&obj_id).unwrap();
                assert!(
                    expected_pos == ref_reader.stream_position().unwrap(),
                    "prop name: {} len: {}, expected_pos: {}, actual_pos: {}",
                    name,
                    prop_len,
                    expected_pos,
                    ref_reader.stream_position().unwrap()
                );

                props.push(Rc::new(prop));

                ref_reader.seek(SeekFrom::Start(expected_pos)).unwrap();
            }
        }
    }
    ent_to_props
}

fn read_unparsed_property_chunk<R: io::Read + io::Seek>(
    toc: &ChunkFileTableOfContents,
    chunk_name: &str,
    reader: &mut R,
) -> Vec<UnparsedProperty> {
    let mut entries = Vec::new();

    if let Some(chunk) = toc.get_chunk(chunk_name.to_owned()) {
        reader.seek(SeekFrom::Start(chunk.offset)).unwrap();

        let end_pos = chunk.offset + chunk.length;
        while reader.stream_position().unwrap() < end_pos {
            let entity_id = read_i32(reader);
            let prop_len = read_u32(reader);
            let expected_pos = reader.stream_position().unwrap() + prop_len as u64;
            reader.seek(SeekFrom::Start(expected_pos)).unwrap();

            entries.push(UnparsedProperty {
                entity_id,
                byte_len: prop_len,
            });
        }
    }

    entries
}

fn read_unparsed_link_data_chunk<R: io::Read + io::Seek>(
    toc: &ChunkFileTableOfContents,
    chunk_name: &str,
    reader: &mut R,
) -> Vec<UnparsedLinkData> {
    let mut entries = Vec::new();

    if let Some(chunk) = toc.get_chunk(chunk_name.to_owned()) {
        reader.seek(SeekFrom::Start(chunk.offset)).unwrap();

        let end_pos = chunk.offset + chunk.length;
        if reader.stream_position().unwrap() < end_pos {
            let data_len = read_u32(reader) as usize;
            while reader.stream_position().unwrap() < end_pos {
                let link_id = read_i32(reader);
                reader.seek(SeekFrom::Current(data_len as i64)).unwrap();
                entries.push(UnparsedLinkData {
                    link_id,
                    byte_len: data_len as u32,
                });
            }
        }
    }

    entries
}
