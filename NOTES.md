
TODO/Issues
1. Agent auth: Claude Code auth will work, however once a sandbox is created, the cli tool doesn't automatiically auth.
2. Port forwarding is done half through podman port forwarding and some through mutagen. Refactor so its only through Mutagen. Also work adding a new rust module for it.
3. Packages: Unable to install node through packages
4. Packages: Come up with a way to install directly through Aqua, which means I can install stuff from Github?
5. All the stad::process:Command() code that forks out to a separate process, deserves to be in some utility class, especially for podman/mutagen/llama calls
