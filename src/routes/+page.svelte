<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Field from "$lib/components/ui/field";
    import { Input } from "$lib/components/ui/input";

    import { invoke } from "@tauri-apps/api/core";

    let name = $state("");
    let greetMsg = $state("");

    async function greet(event: Event) {
        event.preventDefault();
        // Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
        greetMsg = await invoke("greet", { name });
    }
</script>

<main class="m-0 pt-[10vh] flex flex-col items-center justify-center text-center">
    <h1 class="text-3xl">Welcome to Sandbox-mio</h1>

    <nav class="flex gap-3 mt-4">
        <Button href="/intro" variant="outline">Intro</Button>
        <Button href="/projects" variant="outline">Projects</Button>
    </nav>

    <form class="flex flex-col items-center w-full mt-8" onsubmit={greet}>
        <Field.Set class="w-full max-w-sm">
            <Field.Group>
                <Field.Field>
                    <Field.Label>Enter a name</Field.Label>
                    <div class="flex w-full gap-2">
                        <Input
                            id="greet-input"
                            placeholder="Enter a name..."
                            bind:value={name}
                        />
                        <Button type="submit">Greet</Button>
                    </div>
                </Field.Field>
            </Field.Group>
        </Field.Set>
    </form>

    <p>{greetMsg}</p>
</main>
