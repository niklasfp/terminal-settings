// Settings UI runtime (shared by every terminal editor). The page markup (one row per setting) is generated from the
// schema; this file wires every row generically through its data-type and data-role attributes.
"use strict";

(() => {
  const $ = (sel, root = document) => root.querySelector(sel);
  const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

  // ---------- token & API ----------
  const params = new URLSearchParams(location.search);
  if (params.has("t")) {
    sessionStorage.setItem("settings-token", params.get("t"));
    history.replaceState(null, "", location.pathname + location.hash);
  }
  const token = sessionStorage.getItem("settings-token") || "";

  async function api(path, body) {
    const res = await fetch(path, {
      method: body === undefined ? "GET" : "POST",
      headers: { "X-Settings-Token": token, ...(body === undefined ? {} : { "Content-Type": "application/json" }) },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    let data = null;
    try { data = await res.json(); } catch { /* empty body */ }
    if (res.status === 403) {
      // The token belongs to an earlier run of the editor; nothing on this page will work until it's reopened.
      const b = document.getElementById("banner");
      b.className = "banner error";
      b.textContent = "This page belongs to an earlier run of the editor, so it can't load or save anything. " +
        "Open the new address the editor printed in the terminal.";
      b.hidden = false;
    }
    if (!res.ok) {
      const err = new Error((data && data.error) || `Request failed (${res.status})`);
      err.status = res.status;
      err.data = data;
      throw err;
    }
    return data;
  }

  // ---------- schema & state ----------
  const schema = JSON.parse($("#rio-schema").textContent);
  const S = schema.settings;
  const T = (schema.app && schema.app.terminal) || "The terminal";
  // Some terminals (Rio) ignore [colors] while a theme is set; others layer colors on top.
  const themeLocksColors = !!(schema.app && schema.app.themeLocksColors);
  const rows = new Map($$(".setting").map((el) => [el.dataset.key, el]));
  const controllers = new Map();

  let state = { values: {}, raw: {}, version: null };
  let scope = "";
  const dirty = new Map(); // key -> value (null = remove the line)

  const scopedKey = (key) => (scope ? `platform.${scope}.${key}` : key);
  const fileValue = (key) => {
    if (S[key].t === "raw") return scope ? undefined : (state.raw[key] || "") || undefined;
    const v = state.values[scopedKey(key)];
    return v === undefined ? undefined : v;
  };
  // What the setting resolves to if this scope doesn't set it.
  const fallbackValue = (key) => {
    if (scope && state.values[key] !== undefined) return state.values[key];
    return S[key].d;
  };
  const currentValue = (key) => (dirty.has(key) ? (dirty.get(key) ?? fallbackValue(key)) : (fileValue(key) ?? fallbackValue(key)));
  const isSet = (key) => (dirty.has(key) ? dirty.get(key) !== null : fileValue(key) !== undefined);

  const same = (a, b) => JSON.stringify(norm(a)) === JSON.stringify(norm(b));
  function norm(v) {
    if (Array.isArray(v)) return v.map(norm);
    if (v && typeof v === "object") return Object.fromEntries(Object.keys(v).sort().map((k) => [k, norm(v[k])]));
    if (typeof v === "string" && /^#[0-9a-f]{6,8}$/i.test(v)) return v.toLowerCase();
    return v;
  }

  // ---------- formatting ----------
  function describe(key, v) {
    const s = S[key];
    if (v === null || v === undefined || v === "") return s.t === "theme" || s.t === "string" ? "empty" : `${T} decides`;
    switch (s.t) {
      case "bool": return v ? "On" : "Off";
      case "margin": return v.join(" ") + " px";
      case "pair": return `${v[0]} / ${v[1]} ms`;
      case "strings": case "fonts": case "flags": return v.length ? v.join(", ") : "none";
      case "records": return v.length ? `${v.length} entr${v.length === 1 ? "y" : "ies"}` : "none";
      case "blur": return v === false ? "Off" : v === true ? "Blur" : v;
      case "fontStyle": return v === false ? "Off" : v;
      case "raw": return `${T}'s built-in rules`;
      default: return String(v);
    }
  }

  // ---------- controllers (one per data-type) ----------
  function controllerFor(row) {
    const key = row.dataset.key;
    const s = S[key];
    const ctl = $(".control", row);
    const valueEl = $('[data-role="value"]', ctl);
    const changed = (v) => onEdit(key, v);

    switch (s.t) {
      case "bool":
        if (valueEl.type === "checkbox") {
          valueEl.addEventListener("change", () => changed(valueEl.checked));
          return { set: (v) => { valueEl.checked = !!v; } };
        }
        // Optional on/off without a default renders as On / Off buttons; handled like a segmented choice.
      // falls through
      case "enum":
      case "blur": {
        if (valueEl.tagName === "SELECT") {
          const empty = new Option("—", "");
          empty.disabled = true;
          valueEl.addEventListener("change", () => changed(valueEl.value));
          return {
            set: (v) => {
              if (v == null || !s.v.includes(v)) { if (!empty.parentNode) valueEl.prepend(empty); valueEl.value = ""; }
              else { empty.remove(); valueEl.value = v; }
            },
          };
        }
        const buttons = $$("button", valueEl);
        const valueOf = (b) => ("json" in b.dataset ? JSON.parse(b.dataset.json) : b.dataset.value);
        buttons.forEach((b) => b.addEventListener("click", () => changed(valueOf(b))));
        radioKeys(valueEl, (b) => changed(valueOf(b)));
        return {
          set: (v) => {
            buttons.forEach((b) => b.setAttribute("aria-checked", String(same(valueOf(b), v))));
            rovingTabindex(valueEl);
          },
        };
      }
      case "int":
      case "float": {
        const slider = $('[data-role="slider"]', ctl);
        const parse = () => {
          const raw = valueEl.value.trim();
          if (raw === "") return null;
          const n = Number(raw);
          return Number.isFinite(n) ? n : NaN;
        };
        valueEl.addEventListener("input", () => {
          const n = parse();
          if (slider && Number.isFinite(n)) slider.value = n;
          if (!Number.isNaN(n)) changed(n);
        });
        slider?.addEventListener("input", () => { valueEl.value = slider.value; changed(Number(slider.value)); });
        return {
          set: (v) => {
            if (document.activeElement !== valueEl) valueEl.value = v ?? "";
            if (slider) slider.value = v ?? slider.min;
            valueEl.placeholder = v == null ? "auto" : "";
          },
        };
      }
      case "string": case "path": case "font": case "theme": {
        valueEl.addEventListener("input", () => changed(valueEl.value === "" ? null : valueEl.value));
        return { set: (v) => { if (document.activeElement !== valueEl) valueEl.value = v ?? ""; } };
      }
      case "color": {
        const picker = $('[data-role="picker"]', ctl);
        const swatch = $(".swatch", ctl);
        colorInputs.push(valueEl);
        const paint = (v) => paintSwatch(swatch, picker, v, s.v);
        valueEl.addEventListener("input", () => { paint(valueEl.value); changed(valueEl.value === "" ? null : valueEl.value); });
        picker.addEventListener("input", () => {
          const alpha = /^#?[0-9a-f]{8}$/i.test(valueEl.value) ? valueEl.value.slice(-2) : "";
          valueEl.value = picker.value + alpha;
          paint(valueEl.value);
          changed(valueEl.value);
        });
        return { set: (v) => { if (document.activeElement !== valueEl) valueEl.value = v ?? ""; paint(v); } };
      }
      case "strings":
      case "fonts": {
        const list = $('[data-role="chip-list"]', ctl);
        const input = $('[data-role="chip-input"]', ctl);
        let items = [];
        const render = () => {
          list.replaceChildren(...items.map((text, i) => {
            const chip = document.createElement("span");
            chip.className = "chip";
            chip.textContent = text;
            const x = document.createElement("button");
            x.type = "button";
            x.textContent = "×";
            x.title = "Remove";
            x.addEventListener("click", () => { items.splice(i, 1); render(); changed([...items]); });
            chip.append(x);
            return chip;
          }));
        };
        const commit = () => {
          const v = input.value.trim();
          if (!v) return;
          items.push(v);
          input.value = "";
          render();
          changed([...items]);
        };
        input.addEventListener("keydown", (e) => {
          if (e.key === "Enter" || (e.key === "," && s.t === "strings" && !input.value.includes("="))) { e.preventDefault(); commit(); }
          else if (e.key === "Backspace" && input.value === "" && items.length) { items.pop(); render(); changed([...items]); }
        });
        input.addEventListener("blur", commit);
        input.addEventListener("valuepick", commit);
        return { set: (v) => { items = Array.isArray(v) ? [...v] : []; render(); } };
      }
      case "flags": {
        const boxes = $$("input[data-flag]", ctl);
        boxes.forEach((b) => b.addEventListener("change", () => changed(boxes.filter((x) => x.checked).map((x) => x.dataset.flag))));
        return { set: (v) => boxes.forEach((b) => { b.checked = Array.isArray(v) && v.includes(b.dataset.flag); }) };
      }
      case "margin": {
        const inputs = $$("input[data-i]", ctl);
        inputs.forEach((i) => i.addEventListener("input", () => {
          const [t, r, b, l] = inputs.map((x) => Number(x.value || 0));
          changed(t === r && r === b && b === l ? [t] : t === b && r === l ? [t, r] : [t, r, b, l]);
        }));
        return {
          set: (v) => {
            const a = Array.isArray(v) ? v : [0];
            const full = a.length === 1 ? [a[0], a[0], a[0], a[0]] : a.length === 2 ? [a[0], a[1], a[0], a[1]] : a;
            inputs.forEach((x, i) => { if (document.activeElement !== x) x.value = full[i] ?? 0; });
          },
        };
      }
      case "pair": {
        const inputs = $$("input[data-i]", ctl);
        inputs.forEach((i) => i.addEventListener("input", () => changed(inputs.map((x) => Number(x.value || 0)))));
        return { set: (v) => inputs.forEach((x, i) => { if (document.activeElement !== x) x.value = Array.isArray(v) ? v[i] : ""; }) };
      }
      case "fontStyle": {
        const off = $('[data-role="disabled"]', ctl);
        const emit = () => changed(off.checked ? false : valueEl.value.trim() || "default");
        valueEl.addEventListener("input", emit);
        off.addEventListener("change", () => { valueEl.disabled = off.checked; emit(); });
        return {
          set: (v) => {
            off.checked = v === false;
            valueEl.disabled = v === false;
            if (document.activeElement !== valueEl) valueEl.value = v === false || v === "default" ? "" : v ?? "";
          },
        };
      }
      case "records": {
        const list = $('[data-role="record-list"]', ctl);
        const fields = s.f || [];
        let items = [];
        const emit = () => changed(items.map((it) => Object.fromEntries(Object.entries(it).filter(([, v]) => v !== "" && v != null))));
        const render = () => {
          list.replaceChildren(...items.map((item, idx) => {
            const rec = document.createElement("div");
            rec.className = "record";
            rec.style.setProperty("--cols", fields.length);
            for (const f of fields) {
              const label = document.createElement("label");
              label.append(f.l);
              const input = document.createElement("input");
              input.type = "text";
              input.spellcheck = false;
              input.autocomplete = "off";
              if (f.t === "font") fontPicker(input);
              if (f.list) input.setAttribute("list", f.list);
              if (f.t === "color" || f.k === "esc" || f.k === "start" || f.k === "end") input.classList.add("mono");
              input.placeholder = f.p || "";
              input.value = item[f.k] ?? "";
              input.addEventListener("input", () => { item[f.k] = input.value; emit(); });
              label.append(input);
              if (f.t === "color") attachSwatch(input, (v) => { item[f.k] = v; emit(); });
              rec.append(label);
            }
            const rm = document.createElement("button");
            rm.type = "button";
            rm.className = "remove";
            rm.title = "Remove";
            rm.textContent = "×";
            rm.addEventListener("click", () => { items.splice(idx, 1); render(); emit(); });
            rec.append(rm);
            return rec;
          }));
        };
        $('[data-role="add"]', ctl).addEventListener("click", () => {
          items.push({});
          render();
          $$("input", list.lastElementChild)[0]?.focus();
        });
        return {
          set: (v) => {
            if (list.contains(document.activeElement)) return;
            items = Array.isArray(v) ? v.map((x) => ({ ...x })) : [];
            render();
          },
        };
      }
      case "raw": {
        valueEl.addEventListener("input", () => changed(valueEl.value.trim() === "" ? null : valueEl.value));
        return { set: (v) => { if (document.activeElement !== valueEl) valueEl.value = v ?? ""; } };
      }
      default:
        return { set: () => {} };
    }
  }

  // ---------- color swatches ----------
  // Shows hex colors, CSS/X11 names ("red") and keywords such as cell-foreground (striped marker).
  function colorToHex(v) {
    if (typeof v !== "string" || !v.trim()) return null;
    const t = v.trim();
    const named = colorNames.find((c) => c.name.toLowerCase() === t.toLowerCase() || c.name.replace(/\s+/g, "").toLowerCase() === t.replace(/\s+/g, "").toLowerCase());
    if (named) return named.hex;
    if (/^#?[0-9a-f]{6}([0-9a-f]{2})?$/i.test(t)) return (t.startsWith("#") ? t : "#" + t).slice(0, 7).toLowerCase();
    if (!CSS.supports("color", t)) return null;
    const probe = document.createElement("canvas").getContext("2d");
    probe.fillStyle = "#000"; probe.fillStyle = t;
    return /^#[0-9a-f]{6}$/i.test(probe.fillStyle) ? probe.fillStyle : null;
  }
  function paintSwatch(swatch, picker, v, keywords) {
    const hex = colorToHex(v);
    const keyword = !hex && typeof v === "string" && (keywords || []).includes(v.trim());
    swatch.style.setProperty("--swatch", hex || "transparent");
    swatch.classList.toggle("empty", !hex && !keyword);
    swatch.classList.toggle("keyword", keyword);
    swatch.title = keyword ? v.trim() : "";
    if (hex) picker.value = hex;
  }
  // A swatch + native picker in front of a text field (used inside list rows).
  function attachSwatch(input, onPick) {
    const swatch = document.createElement("span");
    swatch.className = "swatch small";
    const picker = document.createElement("input");
    picker.type = "color";
    picker.setAttribute("aria-label", "Pick color");
    swatch.append(picker);
    colorInputs.push(input);
    if (colorNames.length) suggestPicker(input, colorSource(input));
    const wrap = document.createElement("div");
    wrap.className = "color";
    input.replaceWith(wrap);
    wrap.append(swatch, input);
    const repaint = () => paintSwatch(swatch, picker, input.value);
    input.addEventListener("input", repaint);
    picker.addEventListener("input", () => { input.value = picker.value; repaint(); onPick(picker.value); });
    repaint();
  }

  // ---------- suggestion picker: one combobox for fonts and named colors ----------
  // Native <datalist> filters by the field's current text and behaves differently per input,
  // so a field holding "FiraCode Nerd Font Mono" would only ever offer that one font.
  // This picker always opens on the full list (current value highlighted); typing filters.
  let fontNames = [];
  let colorNames = []; // [{ name, hex }] — only for terminals that accept X11 color names
  async function loadFonts() {
    try { fontNames = await api("/api/fonts"); } catch { /* optional: fields still accept any name */ }
  }
  async function loadColorNames() {
    try {
      const lines = await api("/api/list/x11-colors");
      const seen = new Set();
      for (const line of lines) {
        const m = /^(.+?)\s*=\s*(#[0-9a-f]{6})$/i.exec(line);
        if (!m) continue;
        // Ghostty lists "alice blue" and "AliceBlue"; keep one of each.
        const norm = m[1].replace(/\s+/g, "").toLowerCase();
        if (seen.has(norm)) continue;
        seen.add(norm);
        colorNames.push({ name: m[1], hex: m[2].toLowerCase() });
      }
      if (colorNames.length) for (const input of colorInputs) suggestPicker(input, colorSource(input));
    } catch { /* this terminal doesn't take color names */ }
  }

  const fontSource = {
    empty: () => (fontNames.length ? "No matching fonts. Press Enter to use what you typed." : "Font list unavailable; type a name."),
    items: (q) => fontNames.filter((f) => !q || f.toLowerCase().includes(q)).map((f) => ({ value: f })),
    render: (item) => {
      const preview = document.createElement("span");
      preview.className = "preview";
      preview.textContent = item.value;
      preview.style.fontFamily = `"${item.value.replace(/["\\]/g, "")}", var(--font-fallback)`;
      const plain = document.createElement("span");
      plain.className = "plain";
      plain.textContent = item.value;
      return [preview, plain];
    },
  };

  // Named colors plus the setting's keywords (cell-foreground, bright); hex input closes the list.
  function colorSource(input) {
    const key = input.closest(".setting")?.dataset.key;
    const keywords = (key && S[key].t === "color" && S[key].v) || [];
    return {
      empty: () => "No matching color names. Hex values like #1e1e2e work too.",
      items: (q) => {
        if (q.startsWith("#")) return [];
        const kw = keywords.filter((k) => !q || k.includes(q)).map((k) => ({ value: k, keyword: true }));
        const named = colorNames.filter((c) => !q || c.name.toLowerCase().includes(q)).map((c) => ({ value: c.name, hex: c.hex }));
        return [...kw, ...named];
      },
      render: (item) => {
        const chip = document.createElement("span");
        chip.className = "color-chip" + (item.keyword ? " keyword" : "");
        if (item.hex) chip.style.background = item.hex;
        const name = document.createElement("span");
        name.className = "preview";
        name.textContent = item.value;
        const hex = document.createElement("span");
        hex.className = "plain";
        hex.textContent = item.keyword ? "keyword" : item.hex;
        return [chip, name, hex];
      },
    };
  }

  // Themes: installed ones first, then (Rio) the catalogue, installed on pick. Each shows a preview tile.
  let themes = [];
  let canInstallThemes = false;
  async function loadThemes() {
    try {
      const r = await api("/api/themes");
      themes = r.themes || [];
      canInstallThemes = !!r.canInstall;
    } catch { /* fields still accept any name */ }
  }
  const themeSource = {
    empty: () => (themes.length ? "No matching themes." : "No themes found; type a name."),
    items: (q) => themes.filter((t) => !q || t.name.toLowerCase().includes(q)).map((t) => ({ value: t.name, theme: t })),
    render: (item) => {
      const t = item.theme;
      const tile = document.createElement("span");
      tile.className = "theme-tile";
      tile.style.background = t.bg || "transparent";
      tile.style.color = t.fg || "inherit";
      const aa = document.createElement("span");
      aa.className = "aa";
      aa.textContent = "Aa";
      const dots = document.createElement("span");
      dots.className = "dots";
      for (const c of (t.ansi || []).slice(0, 8)) {
        const d = document.createElement("i");
        d.style.background = c || "transparent";
        dots.append(d);
      }
      tile.append(aa, dots);
      const name = document.createElement("span");
      name.className = "preview";
      name.textContent = t.name;
      const tag = document.createElement("span");
      tag.className = "plain";
      tag.textContent = t.installed ? "installed" : "download";
      return [tile, name, tag];
    },
    prepare: async (item) => {
      const t = item.theme;
      if (!t || t.installed) return true;
      if (!canInstallThemes) return true;
      toast(`Downloading ${t.name}…`);
      try {
        await api("/api/themes/install", { name: t.name });
        t.installed = true;
        toast(`Installed ${t.name}. Save to switch to it.`);
        return true;
      } catch (err) {
        toast(err.message);
        return false;
      }
    },
  };

  const colorInputs = [];
  const popover = document.createElement("div");
  popover.className = "suggest-popover";
  popover.id = "suggest-popover";
  popover.setAttribute("role", "listbox");
  popover.hidden = true;
  document.body.append(popover);
  const sources = new WeakMap();
  let pickerInput = null;
  let shown = [];
  let active = -1;

  function suggestPicker(input, source) {
    if (sources.has(input)) { sources.set(input, source); return; }
    sources.set(input, source);
    input.removeAttribute("list"); // the picker replaces any native suggestion list
    input.setAttribute("role", "combobox");
    input.setAttribute("aria-autocomplete", "list");
    input.setAttribute("aria-expanded", "false");
    input.setAttribute("aria-controls", popover.id);
    input.addEventListener("click", () => { if (popover.hidden || pickerInput !== input) openPicker(input, ""); });
    input.addEventListener("input", (e) => { if (e.isTrusted) openPicker(input, input.value); });
    // Capture phase so Enter picks before the field's own Enter handling (e.g. add chip).
    input.addEventListener("keydown", (e) => {
      const open = !popover.hidden && pickerInput === input;
      if (!open) {
        if (e.key === "ArrowDown") { e.preventDefault(); openPicker(input, ""); }
        return;
      }
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        moveActive(e.key === "ArrowDown" ? 1 : -1);
      } else if (e.key === "Enter" && active >= 0) {
        e.preventDefault();
        e.stopImmediatePropagation();
        pickValue(shown[active]);
      } else if (e.key === "Escape") {
        e.stopPropagation();
        closePicker();
      }
    }, true);
    input.addEventListener("blur", () => setTimeout(() => { if (pickerInput === input) closePicker(); }, 150));
  }
  const fontPicker = (input) => suggestPicker(input, fontSource);

  function openPicker(input, query) {
    const source = sources.get(input);
    if (!source) return;
    pickerInput = input;
    shown = source.items(query.trim().toLowerCase());
    if (!shown.length && query.trim().startsWith("#")) { closePicker(); return; }
    active = shown.length ? Math.max(0, shown.findIndex((i) => i.value === input.value)) : -1;
    if (!shown.length) {
      const empty = document.createElement("div");
      empty.className = "suggest-empty";
      empty.textContent = source.empty();
      popover.replaceChildren(empty);
    } else {
      popover.replaceChildren(...shown.map((item, i) => {
        const option = document.createElement("div");
        option.className = "suggest-option";
        option.id = "suggest-opt-" + i;
        option.setAttribute("role", "option");
        option.append(...source.render(item));
        // mousedown + preventDefault keeps focus in the field.
        option.addEventListener("mousedown", (e) => { e.preventDefault(); pickValue(item); });
        return option;
      }));
    }
    positionPicker();
    popover.hidden = false;
    input.setAttribute("aria-expanded", "true");
    updateActive();
  }

  function positionPicker() {
    if (!pickerInput) return;
    const r = pickerInput.getBoundingClientRect();
    popover.style.left = `${r.left + window.scrollX}px`;
    popover.style.top = `${r.bottom + window.scrollY + 4}px`;
    popover.style.minWidth = `${Math.max(r.width, 280)}px`;
  }

  function moveActive(step) {
    if (!shown.length) return;
    active = (active + step + shown.length) % shown.length;
    updateActive();
  }

  function updateActive() {
    $$(".suggest-option", popover).forEach((o, i) => o.setAttribute("aria-selected", String(i === active)));
    const current = active >= 0 ? document.getElementById("suggest-opt-" + active) : null;
    if (current) {
      pickerInput?.setAttribute("aria-activedescendant", current.id);
      current.scrollIntoView({ block: "nearest" });
    } else {
      pickerInput?.removeAttribute("aria-activedescendant");
    }
  }

  async function pickValue(item) {
    const input = pickerInput;
    const source = input && sources.get(input);
    closePicker();
    if (!input) return;
    // Sources can do work first (e.g. download a theme); false means "don't pick".
    if (source?.prepare && !(await source.prepare(item))) return;
    input.value = item.value;
    // Plain fields react to "input"; list fields (chips) add the value on "valuepick".
    input.dispatchEvent(new Event("input"));
    input.dispatchEvent(new Event("valuepick"));
  }

  function closePicker() {
    popover.hidden = true;
    pickerInput?.setAttribute("aria-expanded", "false");
    pickerInput?.removeAttribute("aria-activedescendant");
    pickerInput = null;
  }

  window.addEventListener("scroll", (e) => { if (e.target !== popover && !popover.hidden) positionPicker(); }, true);
  window.addEventListener("resize", () => { if (!popover.hidden) positionPicker(); });

  // ---------- radio groups (WAI-ARIA: one tab stop, arrow keys move and select) ----------
  function rovingTabindex(group) {
    const buttons = $$('[role="radio"]', group);
    const checked = buttons.find((b) => b.getAttribute("aria-checked") === "true") || buttons[0];
    buttons.forEach((b) => { b.tabIndex = b === checked ? 0 : -1; });
  }
  function radioKeys(group, select) {
    group.addEventListener("keydown", (e) => {
      const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
      if (!step) return;
      e.preventDefault();
      const buttons = $$('[role="radio"]', group);
      const i = buttons.indexOf(document.activeElement);
      const next = buttons[(i + step + buttons.length) % buttons.length];
      select(next);
      next.focus();
    });
  }

  // ---------- edits & rendering ----------
  function onEdit(key, value) {
    const file = fileValue(key);
    if (value === null) {
      if (file === undefined) dirty.delete(key);
      else dirty.set(key, null);
    } else if (file !== undefined ? same(value, file) : same(value, fallbackValue(key)) && S[key].t !== "raw") {
      // Back to what the file has, or to the default while unset: nothing to save.
      dirty.delete(key);
    } else {
      dirty.set(key, value);
    }
    clearError(key);
    renderRow(key, { keepFocus: true });
    renderChrome();
    if (key.startsWith("colors.") || key === "theme") renderColorsExtras();
  }

  function renderRow(key) {
    const row = rows.get(key);
    const s = S[key];
    applyLock(key);
    const reset = $('[data-role="reset"]', row);
    row.classList.toggle("is-set", isSet(key));
    row.classList.toggle("is-dirty", dirty.has(key));
    // Reset first undoes an unsaved edit; only an untouched, set value is removed from the file.
    const label = s.l;
    if (dirty.has(key)) {
      reset.dataset.mode = "undo";
      reset.title = "Undo this change";
      reset.setAttribute("aria-label", `Undo change to ${label}`);
    } else {
      reset.dataset.mode = "clear";
      reset.title = "Reset to default (removes the line from config.toml)";
      reset.setAttribute("aria-label", `Reset ${label} to default`);
    }
    const v = currentValue(key);
    controllers.get(key).set(v);

    const usingDefault = !isSet(key);
    $$("input, textarea", $(".control", row)).forEach((el) => el.classList.toggle("is-default", usingDefault && el.type !== "range"));
    $(".segmented, .chips", row)?.classList.toggle("is-default", usingDefault);
    $(".switch", row)?.parentElement.classList.toggle("is-default", usingDefault);

    const def = $('[data-role="default"]', row);
    const fb = fallbackValue(key);
    // Always shown (on the key line), so editing never adds or removes a line and the layout stays put.
    def.textContent = scope && state.values[key] !== undefined
      ? `all platforms: ${describe(key, fb)}`
      : s.t === "raw" ? ""
      : `default: ${describe(key, fb)}`;
  }

  const themeActive = () => { const t = currentValue("theme"); return !!(t && String(t).length); };
  const platformNames = { macos: "macOS", linux: "Linux", windows: "Windows" };

  function renderAll() {
    for (const key of rows.keys()) renderRow(key);
    renderChrome();
    renderColorsExtras();
    const sb = $("#scope-banner");
    sb.hidden = scope === "";
    if (scope) {
      sb.textContent = `Editing ${platformNames[scope]} overrides ([platform.${scope}] in config.toml). ` +
        "Dimmed settings can't be overridden per platform; switch “Applies to” back to All platforms to edit them.";
    }
  }

  function renderChrome() {
    const n = dirty.size;
    $("#save").disabled = n === 0;
    const count = $("#dirty-count");
    count.hidden = n === 0;
    count.textContent = n;
    const scopeSel = $("#scope");
    if (scopeSel) {
      scopeSel.disabled = n > 0;
      scopeSel.title = n > 0 ? "Save or undo your changes before switching" : "";
    }

    const perGroup = {};
    for (const [key] of rows) if (isSet(key)) perGroup[S[key].g] = (perGroup[S[key].g] || 0) + 1;
    $$("[data-modified-count]").forEach((el) => { el.textContent = perGroup[el.dataset.modifiedCount] || ""; });
    document.title = (n ? "• " : "") + (schema.app?.title || "Settings");
  }

  // ---------- colors extras ----------
  function renderColorsExtras() {
    const notice = $("#colors-theme-notice");
    if (notice) notice.hidden = !(themeLocksColors && themeActive());
    // Colors lock/unlock with the theme, so re-render those rows.
    for (const key of rows.keys()) if (S[key].g === "colors") applyLock(key);
    const pv = $("#terminal-preview");
    if (!pv) return;
    const c = (name) => currentValue("colors." + name) || "transparent";
    const ansi = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
    pv.style.background = c("background");
    pv.style.color = c("foreground");
    const span = (color, text) => `<span style="color:${esc(c(color))}">${text}</span>`;
    pv.innerHTML =
      `<div>${span("green", "➜")} ${span("cyan", "~/rio")} ${span("blue", "git:(")}${span("red", "main")}${span("blue", ")")} ls -la</div>` +
      `<div>${span("blue", "drwxr-xr-x")} ${span("yellow", "settings")}  ${span("magenta", "config.toml")}  ` +
      `<span class="sel" style="background:${esc(c("selection-background"))};color:${esc(c("selection-foreground"))}">selected text</span> ` +
      `<span class="cur" style="background:${esc(c("cursor"))}"></span></div>` +
      `<div class="row">${ansi.map((a) => `<span class="chip-c" title="${a}" style="background:${esc(c(a))}"></span>`).join("")}</div>` +
      `<div class="row">${ansi.map((a) => `<span class="chip-c" title="light-${a}" style="background:${esc(c("light-" + a))}"></span>`).join("")}</div>`;
  }
  const esc = (s) => String(s).replace(/[<>"'&]/g, "");

  function applyLock(key) {
    const row = rows.get(key);
    const s = S[key];
    const scopeLocked = scope !== "" && (!s.o || s.t === "raw");
    const themeLocked = themeLocksColors && s.g === "colors" && themeActive();
    const locked = scopeLocked || themeLocked;
    row.classList.toggle("locked", locked);
    row.title = scopeLocked ? "This setting can't be overridden per platform"
      : themeLocked ? `${T} ignores colors while a theme is set` : "";
    // inert also takes the controls out of the tab order and the accessibility tree.
    $(".control", row).inert = locked;
    $('[data-role="reset"]', row).inert = locked;
  }

  // ---------- errors ----------
  function setError(key, message) {
    const row = rows.get(key);
    if (!row) return;
    row.classList.add("has-error");
    const el = $('[data-role="error"]', row);
    el.textContent = message;
    el.hidden = false;
    for (const input of $$("input, select, textarea", $(".control", row))) {
      input.setAttribute("aria-invalid", "true");
      input.setAttribute("aria-describedby", el.id);
    }
  }
  function clearError(key) {
    const row = rows.get(key);
    if (!row) return;
    row.classList.remove("has-error");
    $('[data-role="error"]', row).hidden = true;
    for (const input of $$("[aria-invalid]", row)) {
      input.removeAttribute("aria-invalid");
      input.removeAttribute("aria-describedby");
    }
  }
  function showErrors(err) {
    const errors = err.data && err.data.errors;
    if (!errors) return false;
    let first = null;
    for (const [k, m] of Object.entries(errors)) {
      if (rows.has(k)) { setError(k, m); first ??= k; }
      else toast(m);
    }
    if (first) reveal(first);
    return true;
  }

  // ---------- save flow ----------
  const dialog = $("#save-dialog");
  const changesPayload = () => ({ version: state.version, scope, changes: Object.fromEntries(dirty) });

  async function startSave() {
    if (dirty.size === 0) return;
    try {
      const preview = await api("/api/preview", changesPayload());
      if (!preview.changed) { dirty.clear(); renderAll(); toast("Nothing to save"); return; }
      renderDiff(preview.diff);
      $("#save-error").hidden = true;
      dialog.showModal();
      $("#confirm-save").focus();
    } catch (err) {
      if (err.status === 409) return conflict();
      if (!showErrors(err)) toast(err.message);
    }
  }

  dialog.addEventListener("close", async () => {
    if (dialog.returnValue !== "save") return;
    try {
      state = await api("/api/save", changesPayload());
      dirty.clear();
      renderAll();
      toast(state.note || "Saved.");
    } catch (err) {
      if (err.status === 409) return conflict();
      if (!showErrors(err)) toast(err.message);
    }
  });

  function renderDiff(diff) {
    const pre = $("#diff");
    pre.replaceChildren(...diff.map((d) => {
      const line = document.createElement("div");
      line.className = d.o === "+" ? "add" : d.o === "-" ? "del" : d.o === "…" ? "gap" : "";
      const n = document.createElement("span");
      n.className = "n";
      // Removed lines have no new line number; the +/- marker lives in the text column only.
      n.textContent = d.o === "…" ? "⋯" : d.o === "-" ? "" : d.n;
      const t = document.createElement("span");
      t.textContent = d.o === "…" ? "" : `${d.o === " " ? " " : d.o} ${d.t}`;
      line.append(n, t);
      return line;
    }));
  }

  // ---------- external changes ----------
  const banner = $("#banner");
  function conflict() {
    banner.className = "banner";
    banner.innerHTML = "";
    banner.append("config.toml changed on disk (maybe you edited it in your editor).");
    const reload = document.createElement("button");
    reload.className = "btn small";
    reload.textContent = "Reload and discard my changes";
    reload.addEventListener("click", async () => { dirty.clear(); banner.hidden = true; await refresh(); });
    banner.append(reload);
    banner.hidden = false;
  }

  async function refresh(fromPoll = false) {
    const next = await api("/api/state");
    // Name the real file in the save dialog (Ghostty's is "config", Rio's "config.toml").
    $("#save-file").textContent = (next.path || "").split(/[\\/]/).pop() || "the config file";
    $("#config-path").textContent = next.target && next.target !== next.path ? `${next.path} → ${next.target}` : next.path;
    const notices = $("#notices");
    if (notices) {
      notices.replaceChildren(...(next.notices || []).map((n) => { const p = document.createElement("p"); p.textContent = n; return p; }));
      notices.hidden = !(next.notices && next.notices.length);
    }
    if (next.parseError) {
      banner.className = "banner error";
      banner.textContent = next.parseError + " Fix the file in your editor; this page updates when it's valid again.";
      banner.hidden = false;
    } else if (banner.classList.contains("error")) {
      banner.hidden = true;
      banner.className = "banner";
    }
    if (fromPoll && state.version && next.version !== state.version) {
      if (dirty.size > 0) { conflict(); return; }
      toast("Reloaded changes from config.toml");
    }
    state = next;
    renderAll();
  }

  // ---------- search ----------
  const search = $("#search");
  search.addEventListener("input", () => {
    const terms = search.value.toLowerCase().split(/\s+/).filter(Boolean);
    let any = false;
    for (const row of rows.values()) {
      const hit = terms.every((t) => row.dataset.search.includes(t));
      row.hidden = !hit;
      any ||= hit;
    }
    $$(".card").forEach((card) => { card.hidden = !$$(".setting", card).some((r) => !r.hidden); });
    $$(".group").forEach((g) => { g.hidden = !$$(".setting", g).some((r) => !r.hidden); });
    $("#empty-search").hidden = any;
    // Keep the sidebar in step: hide sections without hits and show hit counts while searching.
    const searching = terms.length > 0;
    for (const a of $$(".sidebar a")) {
      const group = document.getElementById("g-" + a.dataset.group);
      const hits = group && !group.hidden ? $$(".setting", group).filter((r) => !r.hidden).length : 0;
      a.parentElement.hidden = searching && hits === 0;
      a.classList.toggle("searching", searching);
      a.querySelector(".badge-count").textContent = searching ? String(hits) : "";
    }
    if (!searching) renderChrome();
  });

  // ---------- navigation ----------
  const links = new Map($$(".sidebar a").map((a) => [a.dataset.group, a]));
  const observer = new IntersectionObserver((entries) => {
    for (const e of entries) {
      if (!e.isIntersecting) continue;
      links.forEach((a) => a.classList.remove("active"));
      links.get(e.target.dataset.group)?.classList.add("active");
    }
  }, { rootMargin: "-80px 0px -65% 0px" });
  $$(".group").forEach((g) => observer.observe(g));

  function reveal(key) {
    const row = rows.get(key);
    if (!row) return;
    row.scrollIntoView({ behavior: "smooth", block: "center" });
    row.classList.remove("flash");
    void row.offsetWidth;
    row.classList.add("flash");
  }
  document.addEventListener("click", (e) => {
    const a = e.target.closest('a[href^="#s-"]');
    if (!a) return;
    e.preventDefault();
    const row = document.getElementById(a.getAttribute("href").slice(1));
    if (row) reveal(row.dataset.key);
  });

  // ---------- theme switch (System / Light / Dark) ----------
  const themeButtons = $$("#theme-switch button");
  function applyThemePref(pref) {
    document.documentElement.dataset.themePref = pref;
    if (pref === "system") delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = pref;
    themeButtons.forEach((b) => b.setAttribute("aria-checked", String(b.dataset.value === pref)));
    rovingTabindex($("#theme-switch"));
    localStorage.setItem("settings-theme", pref);
  }
  themeButtons.forEach((b) => b.addEventListener("click", () => applyThemePref(b.dataset.value)));
  radioKeys($("#theme-switch"), (b) => applyThemePref(b.dataset.value));
  applyThemePref(localStorage.getItem("settings-theme") || "system");

  // ---------- platform scope ----------
  const platformNow = /Mac/.test(navigator.platform) ? "macos" : /Win/.test(navigator.platform) ? "windows" : "linux";
  $$(`.badge.platform[data-platform="${platformNow}"]`).forEach((b) => b.classList.add("current"));
  $("#scope")?.addEventListener("change", (e) => { scope = e.target.value; renderAll(); });

  // ---------- reset buttons, keyboard ----------
  for (const [key, row] of rows) {
    controllers.set(key, controllerFor(row));
    $('[data-role="reset"]', row).addEventListener("click", () => {
      if (dirty.has(key)) {
        // Undo: back to what config.toml has (or the default if it isn't set).
        dirty.delete(key);
        clearError(key);
        renderRow(key);
        renderChrome();
        if (S[key].g === "colors" || key === "theme") renderColorsExtras();
      } else {
        onEdit(key, null);
      }
    });
  }
  $$("[data-font-picker]").forEach(fontPicker);
  $$("[data-theme-picker]").forEach((i) => suggestPicker(i, themeSource));
  // "Choose…" buttons: the server opens the native dialog and returns the real path.
  for (const btn of $$('[data-role="browse"]')) {
    btn.addEventListener("click", async () => {
      const row = btn.closest(".setting");
      const target = $('[data-role="value"], [data-role="chip-input"]', row);
      btn.disabled = true;
      try {
        const r = await api("/api/choose", { kind: btn.dataset.pick, prompt: "Choose " + S[row.dataset.key].l.toLowerCase() });
        if (r.path) {
          target.value = r.path;
          target.dispatchEvent(new Event("input"));
          target.dispatchEvent(new Event("valuepick"));
        }
      } catch (err) {
        toast(err.message);
      } finally {
        btn.disabled = false;
      }
    });
  }
  $("#save").addEventListener("click", startSave);
  $("#clear-theme")?.addEventListener("click", () => { onEdit("theme", null); reveal("theme"); });
  document.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") { e.preventDefault(); startSave(); }
    else if (e.key === "/" && !/INPUT|TEXTAREA|SELECT/.test(document.activeElement.tagName)) { e.preventDefault(); search.focus(); }
    else if (e.key === "Escape" && document.activeElement === search) { search.value = ""; search.dispatchEvent(new Event("input")); search.blur(); }
  });
  window.addEventListener("beforeunload", (e) => { if (dirty.size) e.preventDefault(); });
  window.addEventListener("pagehide", () => {
    fetch("/api/bye", { method: "POST", keepalive: true, headers: { "X-Settings-Token": token } }).catch(() => {});
  });

  // ---------- toast ----------
  let toastTimer;
  function toast(text) {
    const el = $("#toast");
    el.textContent = text;
    el.classList.add("show");
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => el.classList.remove("show"), 2800);
  }

  // ---------- boot ----------
  async function fillList(id, path) {
    try {
      const items = await api(path);
      $("#" + id).replaceChildren(...items.map((x) => new Option(x)));
    } catch { /* optional */ }
  }

  refresh().catch((err) => {
    banner.className = "banner error";
    banner.textContent = err.status === 403
      ? "This page lost its access token. Open the address printed by rio-settings in your terminal."
      : "Could not reach rio-settings. Is it still running?";
    banner.hidden = false;
  });
  loadFonts();
  loadColorNames().then(() => renderAll());
  loadThemes();
  fillList("action-list", "/api/list/actions");

  setInterval(() => { if (!document.hidden) refresh(true).catch(() => {}); }, 3000);
  setInterval(() => { if (document.hidden) api("/api/state").catch(() => {}); }, 50000);
  document.addEventListener("visibilitychange", () => { if (!document.hidden) refresh(true).catch(() => {}); });
})();
