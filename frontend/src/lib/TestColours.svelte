<script lang="ts">
  import { Button } from './components/ui/button';
  import type { Dashboard } from './dashboard.svelte';

  let { model }: { model: Dashboard } = $props();
  const colours = [
    ['red', 'Red'],
    ['green', 'Green'],
    ['blue', 'Blue'],
    ['white', 'White'],
    ['warm-white', 'Warm'],
    ['daylight-white', 'Daylight'],
  ];
  const active = $derived(model.status?.calibration_pattern ?? 'off');
  async function pick(value: string) {
    try {
      await model.command(`/api/test-pattern/${value}`);
      await model.refresh();
    } catch {
      /* Shared error banner reports failures. */
    }
  }
</script>

<section class="card test-colours" aria-label="Test colour">
  <h2>Test colour</h2>
  <div class="button-row">
    {#each colours as [value, label] (value)}<Button
        variant={active === value ? 'default' : 'outline'}
        aria-pressed={active === value}
        disabled={model.busy}
        onclick={() => pick(value)}>{label}</Button
      >{/each}
    <Button variant="secondary" disabled={model.busy || active === 'off'} onclick={() => pick('off')}
      >Return to sync</Button
    >
  </div>
</section>
