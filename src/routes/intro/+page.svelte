<script lang="ts">
    import Robot from "$lib/Robot.svelte";
    import Button from "$lib/components/ui/button/button.svelte";
    let robot = $state<Robot>();
</script>

<main>
    <Button href="/" variant="outline" class="m-4">← Back</Button>

    <div class="robot">
      <Robot bind:this={robot}/>
    </div>

</main>


<style>
    .robot {
      width: 220px;
      margin: 2rem auto;
      display: block;
    }
</style>
