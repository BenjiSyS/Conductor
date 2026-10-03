<script lang="ts">
  import { app, attempt, saveSettings, savePrefs } from '../../lib/app.svelte';
  import { roleLabels } from '../../lib/format';

  let globalText = $state(app.settings.instructions);
  let role = $state('coder');
  let provider = $state(app.connected[0]?.id ?? '');
</script>

<h2>Instructions</h2>
<p class="lede">Guidance compiled into requests only where relevant — no duplicated prompt bloat.</p>

<div class="card prec">
  <strong class="small">Precedence (highest first)</strong>
  <ol class="small muted">
    <li>Safety and system rules</li>
    <li>Goal Contract</li>
    <li>Project instructions (<span class="mono">conductor.toml</span> and Project memory)</li>
    <li>Role instructions</li>
    <li>Provider instructions</li>
    <li>Your global instructions</li>
  </ol>
</div>

<div class="field">
  <label class="label" for="ins-global">Global instructions</label>
  <textarea
    id="ins-global"
    class="textarea"
    rows="4"
    bind:value={globalText}
    placeholder="e.g. Prefer small, focused changes. Explain trade-offs briefly."
  ></textarea>
  <div class="row">
    <span class="spacer"></span><button
      class="btn sm"
      onclick={() => attempt(() => saveSettings({ instructions: globalText }), 'Saved')}
      disabled={globalText === app.settings.instructions}>Save</button
    >
  </div>
</div>

<div class="field">
  <label class="label" for="ins-role">Role instructions</label>
  <div class="row">
    <select class="select narrow" bind:value={role} aria-label="Role"
      >{#each Object.entries(roleLabels) as [k, v] (k)}<option value={k}>{v}</option>{/each}</select
    >
  </div>
  <textarea
    id="ins-role"
    class="textarea"
    rows="3"
    value={app.prefs?.role_instructions[role] ?? ''}
    onchange={(e) =>
      app.prefs &&
      attempt(
        () =>
          savePrefs({
            role_instructions: { ...app.prefs!.role_instructions, [role]: (e.target as HTMLTextAreaElement).value },
          }),
        'Saved',
      )}
    placeholder="Extra guidance whenever a model acts in this role"
  ></textarea>
</div>

{#if app.connected.length}
  <div class="field">
    <label class="label" for="ins-prov">Provider instructions</label>
    <div class="row">
      <select class="select narrow" bind:value={provider} aria-label="Provider"
        >{#each app.connected as p (p.id)}<option value={p.id}>{p.name}</option>{/each}</select
      >
    </div>
    <textarea
      id="ins-prov"
      class="textarea"
      rows="3"
      value={app.prefs?.provider_instructions[provider] ?? ''}
      onchange={(e) =>
        app.prefs &&
        attempt(
          () =>
            savePrefs({
              provider_instructions: {
                ...app.prefs!.provider_instructions,
                [provider]: (e.target as HTMLTextAreaElement).value,
              },
            }),
          'Saved',
        )}
      placeholder="Notes that only apply to this provider's models"
    ></textarea>
  </div>
{/if}

<p class="small muted">
  Project instructions and decisions are edited per project from the <button
    class="linkish"
    onclick={() => {
      app.settingsTab = null;
      app.memoryOpen = true;
    }}>Project memory</button
  > panel.
</p>

<style>
  .prec {
    margin-bottom: var(--s5);
  }
  .prec ol {
    margin: var(--s2) 0 0;
    padding-left: 1.3em;
  }
  .narrow {
    width: auto;
    margin-bottom: var(--s2);
  }
  .linkish {
    border: none;
    background: none;
    color: var(--accent);
    font: inherit;
    padding: 0;
    cursor: pointer;
  }
</style>
