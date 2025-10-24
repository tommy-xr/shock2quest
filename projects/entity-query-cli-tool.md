## Entity Query CLI tool

This is a new CLI project under tools:

- tools/dark_query

The `dark_query` bin allows you to query game data, with the following arguments:

- `--mission`, `-m` - load a particular mission
  - If no mission is specified, we just use the game file shock2.gam. Otherwise, we load shock2.gam _and_ the specified mission.

And the follow commands:

- `ls` - list all templates and entities. The template id should be shown, along with the name and prop symname
  -- `--only-unparsed` - show all templates and entities with props or links that we don't currently parse. We should flag cases where the entity itself is fully parsed but something in the hierarchy is not parsed.
- `show <entity-id-or-template-id>` - list details for a particular entity. Show all the props, and follow the inheritance hierarchy (via L$MetaProp). This should render as a tree, showing the full set of metaprops and props at each level (highlighting ones that are overridden). For the purpose of this tool, entities and templates can be considered the same (they really are the same, it's just templates have a negative tempalte id, whereas entities haved a positive one)

- `filter=<prop-or-link-name>`:
  - if `ls`, show only entities that have properties that match the name. Wildcards should be supported. Show the matched property or link name in it.
  - if `entity`, show only properties or links on the entity that match the name
  - Allow for value matching inside the prop - for example, `--filter=P$SymName:*Robot*` should match every entity that has a prop symname with 'robot' int he string

Ultimately we want to be able to show the main data for all entities & templates:

- `P$<PropName>`, like `P$SymName`
- `L$<LinkName>`, like `L$MetaProp`
- `LD$<LinkName>`, like `LD$AIWatchO`

Key files to look at for existing implementation are:

- shock2vr/src/mission/entity_creator.rs - has some inheritance details
- dark/src/ss2_enity_info.rs - the main place we load entity info
- dark/src/properties/mod.rs - all the properties and links

## Key Considerations

- Cross reference with references/darkengine
- We ony load a subset of props right now, it'd be helpful to show ones that we _don't_ parse yet. Especially when we show entity details - along with the full list of properties that we _do_ load, it'd be helpful to show properties and links that aren't yet part of the system. We'd probably ahve to enhance ss2_entity_info.rs for htis.

## Use Cases

- Debug an entity that is problematic for a particular mission, like: `cargo run -p dark_query -- --entity=123 --mission=medsci2.msi`
- Debug why certain behavior isn't working. For example, starting at a certain entity, we could follow links back to see why how the behavior is supposed to work
- Debug what entities have certain kinds of links - for example, what entities have a link type of flinderize vs corpse in hydro1.mis?
- Why is the monkeys projectile attack not visible? (Query for a monkey entity, and then query info about the link)
- Usage with LLMs so that they can debug complex relationships

## Proposed Plan

1. [x] Extend ss2*entity_info to also keep a record of all props, links, and link data that we do \_not* parse. We could keep some sort of dictionary of entity id -> unparsed props/links/link data so that we could query this efficiently int he CLI. Implemented in PR #128
2. Add the initial cli scaffolding. This would just be run `cargo run -p dark_query -- --help`
3. Add the `ls` command - test with shock2.gam and a mission
4. Add the `show` command - test with a template in shock2.gam and an entity in amission file
5. Add the `--filter` modifier
