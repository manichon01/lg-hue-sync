<script lang="ts">
  import SettingToggle from './SettingToggle.svelte';
  import { Button } from './components/ui/button';
  import { Link } from '@lucide/svelte';
  import { onMount } from 'svelte';
  import { isTv } from './tv';
  import Fader from './Fader.svelte';
  import type { Dashboard } from './dashboard.svelte';
  let { model }: { model: Dashboard } = $props();
  let hueIp = $state(''),
    area = $state('');
  let areas = $state<{ id: string; name: string }[]>([]),
    areaMessage = $state(''),
    loaded = $state(false);
  let settings = $derived(model.settings);
  async function run(path: string, body?: unknown) {
    try {
      await model.command(path, body);
    } catch {
      /* Shared error banner reports failures. */
    }
  }
  async function loadAreas() {
    try {
      const { request } = await import('./api');
      const result = await request<{
        areas: { id: string; name: string }[];
        selected_area_id: string;
      }>('/api/hue/areas');
      areas = result.areas;
      area = result.selected_area_id;
      areaMessage = `${areas.length} Entertainment Area${areas.length === 1 ? '' : 's'} found.`;
    } catch (error) {
      areaMessage = String(error);
    }
  }
  onMount(() => {
    // TV mode shows every section open, so load the areas without waiting for a toggle.
    if (isTv && !loaded) {
      loaded = true;
      void loadAreas();
    }
  });
  function showDevices(event: Event) {
    if ((event.currentTarget as HTMLDetailsElement).open && !loaded) {
      loaded = true;
      void loadAreas();
    }
  }
  const trimStops = [0, 0.5, 1, 1.5, 2, 2.5];
</script>

<details class="card accordion" ontoggle={showDevices} id="devicesSection" open={isTv}>
  <summary>Devices <span class="muted">Hue</span></summary>
  <div class="section-body">
    <SettingToggle
      title="Follow TV power"
      description="Pause lights in standby and resume when the TV turns on."
      checked={settings?.auto_tv_power ?? true}
      disabled={!settings || model.busy}
      onchange={(checked) => model.patch({ auto_tv_power: checked })}
    />
    <section class="device-subsection">
      <h2>Philips Hue</h2>
      <p class="muted">
        Press the Bridge button, then pair within 45 seconds. Pairing stays on the TV.
      </p>
      <label class="field"
        >Bridge address <input
          bind:value={hueIp}
          placeholder="Leave blank to discover"
          inputmode="decimal"
        /></label
      >
      <Button
        variant="secondary"
        disabled={model.busy}
        onclick={() => run('/api/pair/hue', { ip: hueIp.trim() || null })}
        ><Link aria-hidden="true" />Pair Hue Bridge</Button
      >
      <p class="device-status">
        {model.status?.hue_connected ? 'Connected' : 'Disconnected'}{model.status?.hue_bridge_ip
          ? ` · ${model.status.hue_bridge_ip}`
          : ''}
      </p>
      <label class="field"
        >Entertainment Area <select
          bind:value={area}
          disabled={!areas.length || model.busy}
          onchange={() => run('/api/hue/area', { area_id: area })}
          >{#each areas as item (item.id)}<option value={item.id}>{item.name}</option
            >{/each}</select
        ></label
      >
      <div class="button-row">
        <Button variant="secondary" onclick={loadAreas}>Refresh areas</Button><Button
          variant="secondary"
          disabled={model.busy}
          onclick={() => run('/api/sync-bridge')}>Refresh selected layout</Button
        >
      </div>
      <p class="muted" role="status">{areaMessage}</p>
      <SettingToggle
        title="Hue Entertainment sync"
        description="Release the Hue lights to the Hue app."
        checked={settings?.hue_sync_enabled ?? false}
        disabled={!settings || model.busy}
        onchange={(checked) => model.patch({ hue_sync_enabled: checked })}
      />
      {#if settings}<Fader
          label="Hue output trim"
          value={settings.hue_output_brightness}
          stops={trimStops}
          format={(v) => `${v.toFixed(2)}×`}
          help="Final Hue brightness trim."
          vertical={false}
          onchange={(v) => model.patch({ hue_output_brightness: v })}
          onhelp={() => {}}
          disabled={model.busy}
        />{/if}
    </section>
  </div>
</details>
<details class="card accordion" open={isTv}>
  <summary
    >Calibration
    <a
      class="info-icon"
      href="/calibration-help"
      title="How calibration works: test colours go through the Tuning settings. Click for the full guide."
      aria-label="Calibration help"
      onclick={(event) => event.stopPropagation()}>ⓘ</a
    ></summary
  >
  <div class="section-body">
    <p class="muted">
      Use the Test colour buttons on the Tuning tab: they pass through the Tuning settings, so you
      can adjust saturation or brightness and watch the lights. Click ⓘ for the full guide.
    </p>
    <a class="text-link" href="/capture-patterns" target="_blank" rel="noopener"
      >Open TV capture patterns ↗</a
    >
  </div>
</details>
<details class="card accordion" open={isTv}>
  <summary>Maintenance</summary>
  <div class="section-body">
    <SettingToggle
      title="Bonjour discovery"
      description="Advertise dashboard and API on the local network. Changes save without a restart."
      checked={model.status?.bonjour_enabled ?? false}
      disabled={!model.status || model.busy}
      onchange={(checked) => run('/api/bonjour', { enabled: checked })}
    />
    <Button
      variant="secondary"
      disabled={model.busy}
      onclick={() => {
        if (confirm('Restart capture and lighting pipelines? Output pauses briefly.'))
          void run('/api/restart');
      }}>Restart pipeline</Button
    >
  </div>
</details>
<details class="card accordion">
  <summary>Home Assistant</summary>
  <div class="section-body">
    <p>Open QSync directly on the TV address, then use that address in LG Hue Sync setup.</p>
    <p>Address in this browser: <code>{window.location.hostname}</code></p>
    <a
      class="text-link"
      href="https://my.home-assistant.io/redirect/hacs_repository/?owner=adeze&repository=lg-hue-sync&category=integration"
      target="_blank"
      rel="noopener noreferrer">Open LG Hue Sync in HACS ↗</a
    >
    <a class="text-link" href="/api/status" target="_blank" rel="noopener"
      >Check local API status ↗</a
    >
  </div>
</details>
