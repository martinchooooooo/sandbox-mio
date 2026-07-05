<script lang="ts">
    import Robot from "$lib/Robot.svelte";
    import Button from "$lib/components/ui/button/button.svelte";
    import { m } from "$lib/paraglide/messages.js";
    let robot = $state<Robot>();
</script>

<main>
    <Button href="/" variant="outline" class="m-4">{m.back()}</Button>

    <div class="robot">
      <Robot bind:this={robot}/>
    </div>

</main>


<style>
    .robot {
      max-width: 220px;
      width: 100%;
      margin: 2rem auto;
      display: block;
    }
    @container (max-width: 350px) {
      .robot {
        max-width: 150px;
      }
    }
</style>
