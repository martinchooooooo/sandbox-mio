<script lang="ts">
    import Robot from "$lib/Robot.svelte";
    let robot = $state<Robot>();
</script>

<main>
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
