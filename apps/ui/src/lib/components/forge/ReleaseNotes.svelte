<script>
  import { Check } from "@lucide/svelte";

  /** Parsed notes from `parseNotes`: a summary, What's new, Fixes and closing lines. */
  let { notes, headingLevel = 3 } = $props();
  const heading = $derived(`h${headingLevel}`);
</script>

{#each notes.summary as line}
  <p class="lead">{line}</p>
{/each}

{#if notes.changes.length}
  <section class="group" aria-label="What's new">
    <svelte:element this={heading} class="group-title">What's new</svelte:element>
    <ul>
      {#each notes.changes as change}
        <li><span class="marker" aria-hidden="true"></span>{change}</li>
      {/each}
    </ul>
  </section>
{/if}

{#if notes.fixes.length}
  <section class="group" aria-label="Fixes">
    <svelte:element this={heading} class="group-title">Fixes</svelte:element>
    <ul class="fixes">
      {#each notes.fixes as fix}
        <li><Check size={14} strokeWidth={2.4} aria-hidden="true" />{fix}</li>
      {/each}
    </ul>
  </section>
{/if}

{#each notes.closing as line}
  <p class="closing">{line}</p>
{/each}

<style>
  .lead {
    margin: 0 0 18px;
    color: #eceee9;
    font-size: 0.98rem;
    line-height: 1.55;
    text-wrap: pretty;
  }

  .group {
    margin-bottom: 18px;
  }

  .group-title {
    margin: 0 0 8px;
    color: #d6a04c;
    font-size: 0.7rem;
    font-weight: 750;
    letter-spacing: 0.13em;
    text-transform: uppercase;
  }

  ul {
    display: grid;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr);
    gap: 10px;
    padding: 5px 0;
    color: #c4c9cb;
    font-size: 0.88rem;
    line-height: 1.5;
    text-wrap: pretty;
  }

  .marker {
    width: 6px;
    height: 6px;
    margin: 8px 0 0 5px;
    border-radius: 50%;
    background: #d6a04c;
  }

  .fixes :global(svg) {
    margin-top: 4px;
    color: #5fbf8f;
  }

  .closing {
    margin: 2px 0 14px;
    border-top: 1px solid rgba(255, 255, 255, 0.06);
    padding-top: 14px;
    color: #8e979d;
    font-size: 0.82rem;
    line-height: 1.5;
  }
</style>
