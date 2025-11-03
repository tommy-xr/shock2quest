# Debug Runtime

Currently, it's difficult for LLMs to actually test the running game and iterate.

This project plans to add commands such that an LLM could actually exercise various states of the command and test changes - given it the ability to "play" shock2vr.

## Architecture

This will add a new runtime - the `debug_runtime`. This will be very similar to the `desktop_runtime` - except it will be fully remote controlled via a local HTTP server. Commands will be submitted by a CLI tool `debug_command`

The `debug_command` will `POST` requests to the debug runtime.

## Project Structure

- `tools/debug_command`
- `runtimes/debug_runtime`

## Examples

- Start the server: `cargo run -p debug_runtime -- -m=medsci1.mis --debug-physics`
- Run a command `cargo run -p debug_command -- info`


## Commands

- `info` - show current context:
    - elapsed game time
    - current frame number
    - player position
    - camera position

- `ss <optional-filename.png>` - save a screenshot of the current UI

- `adv <count-or-time>` 
    - `adv` - advance a single frame
    - `adv 10` - advance 10 freames
    - `adv 30s` - advance 30s

- `rc <start-vector-or-entity-id> <dest-point-or-vector-id>`
- `rcf <start-vector-or-entity-id> <distance>`

- `input` - read current input
- `input <channel> <value>` - set input state
- `move <vector>` - teleport to a position

- `ls <optional-count>` - list all entities, sorted by distance to player
- `ent <id>` - list all information about an entity

- `cmd` - run a command (spawn object)


