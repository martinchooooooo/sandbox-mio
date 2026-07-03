<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Field from "$lib/components/ui/field";
    import { Input } from "$lib/components/ui/input";
    import { m } from "$lib/paraglide/messages.js";

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
    <h1 class="text-3xl">{m.home_title()}</h1>

    <nav class="flex gap-3 mt-4">
        <Button href="/intro" variant="outline">{m.nav_intro()}</Button>
        <Button href="/projects" variant="outline">{m.nav_projects()}</Button>
        <Button href="/models" variant="outline">{m.nav_models()}</Button>
        <Button href="/settings" variant="outline">{m.nav_settings()}</Button>
        <Button href="/credentials" variant="outline">{m.nav_credentials()}</Button>
    </nav>

    <form class="flex flex-col items-center w-full mt-8" onsubmit={greet}>
        <Field.Set class="w-full max-w-sm">
            <Field.Group>
                <Field.Field>
                    <Field.Label>{m.greet_label()}</Field.Label>
                    <div class="flex w-full gap-2">
                        <Input
                            id="greet-input"
                            placeholder={m.greet_placeholder()}
                            bind:value={name}
                        />
                        <Button type="submit">{m.greet_button()}</Button>
                    </div>
                </Field.Field>
            </Field.Group>
        </Field.Set>
    </form>

    <p>{greetMsg}</p>
</main>
