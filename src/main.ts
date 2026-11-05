import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save, ask } from "@tauri-apps/plugin-dialog";

type DiskInfo = {
  id: string;
  name: string;
  path: string;
  sizeBytes: number;
  model?: string | null;
  isRemovable: boolean;
  isInternal: boolean;
  deviceType: string;
};

type ImageJob = {
  id: string;
  sourcePath: string;
  imagePath: string;
  mapPath: string;
  sourceName: string;
  sourceSize: number;
  status: string;
  forceOverwrite: boolean;
  lastError?: string | null;
};

type Progress = {
  jobId: string;
  rescuedBytes: number;
  errBytes: number;
  nonTriedBytes: number;
  percent: number;
  currentPos: number;
  status: string;
  message: string;
};

type RecoverableEntry = {
  id: string;
  name: string;
  path: string;
  size: number;
  isDir: boolean;
  fsType: string;
  deleted: boolean;
  locator: string;
};

type Listing = {
  fsType: string;
  entries: RecoverableEntry[];
  message: string;
};

type CloneValidation = {
  ok: boolean;
  errors: string[];
  warnings: string[];
  freeBytes?: number | null;
  requiredBytes: number;
  needsInternalConfirm: boolean;
  needsForceConfirm: boolean;
  summary: string;
};

type PrivilegeChecklist = {
  os: string;
  title: string;
  steps: string[];
  notes: string[];
};

type AppMeta = {
  name: string;
  version: string;
  license: string;
  ddrescueLicense: string;
  filesystems: string[];
  notSupported: string[];
  logPath?: string | null;
};

type Screen = "home" | "clone" | "progress" | "explore" | "about";

const state = {
  screen: "home" as Screen,
  disks: [] as DiskInfo[],
  selectedSource: null as DiskInfo | null,
  imagePath: "",
  forceOverwrite: false,
  confirmInternal: false,
  confirmForce: false,
  validation: null as CloneValidation | null,
  privileges: null as PrivilegeChecklist | null,
  meta: null as AppMeta | null,
  job: null as ImageJob | null,
  progress: null as Progress | null,
  logs: [] as string[],
  listing: null as Listing | null,
  carved: [] as RecoverableEntry[],
  selectedLocators: new Set<string>(),
  error: "",
  busy: false,
};

function $(sel: string, root: ParentNode = document) {
  return root.querySelector(sel);
}

function fmt(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v.toFixed(2)} ${units[i]}`;
}

async function friendlyError(e: unknown): Promise<string> {
  const raw = String(e);
  try {
    return await invoke<string>("map_error_message", { message: raw });
  } catch {
    return raw;
  }
}

function render() {
  const app = document.getElementById("app");
  if (!app) return;

  const stepIndex =
    state.screen === "home"
      ? 0
      : state.screen === "clone"
        ? 1
        : state.screen === "progress"
          ? 2
          : state.screen === "explore"
            ? 3
            : -1;

  const version = state.meta?.version ? `v${state.meta.version}` : "";

  app.innerHTML = `
    <header class="topbar">
      <div class="brand">
        <img src="/app-icon.png" alt="Sectora" />
        <div>
          <h1>Sectora ${escapeHtml(version)}</h1>
          <span>Imagen con ddrescue · recuperación de archivos</span>
        </div>
      </div>
      <div class="steps">
        ${["Inicio", "Clonar", "Progreso", "Explorar"]
          .map(
            (label, i) =>
              `<span class="step-pill ${i === stepIndex ? "active" : i < stepIndex && stepIndex >= 0 ? "done" : ""}">${label}</span>`,
          )
          .join("")}
        <button class="step-pill about-btn" id="go-about" type="button">Acerca de</button>
      </div>
    </header>
    <main class="view">
      ${state.error ? `<div class="error">${escapeHtml(state.error)}</div>` : ""}
      ${viewForScreen()}
    </main>
  `;

  bindEvents();
}

function viewForScreen(): string {
  switch (state.screen) {
    case "home":
      return homeView();
    case "clone":
      return cloneView();
    case "progress":
      return progressView();
    case "explore":
      return exploreView();
    case "about":
      return aboutView();
  }
}

function homeView(): string {
  const priv = state.privileges;
  return `
    <div class="hero-warn">
      Si el disco ya está fallando, <strong>no esperes</strong> a terminar esta app:
      clona ahora con <code>ddrescue</code> hacia un disco sano. Sectora usa el mismo enfoque
      (imagen + mapfile con pausa/reanudación). APFS/HFS+ no están soportados en v1.
    </div>
    <section class="panel">
      <h2>Bienvenido</h2>
      <p class="lead">
        Flujo recomendado: 1) clonar el disco dañado a una imagen en otra unidad,
        2) explorar FAT32 / exFAT / NTFS o hacer carving, 3) exportar a un destino sano.
        En macOS solo se soporta Apple Silicon.
      </p>
      <div class="actions">
        <button class="btn" id="go-clone">Nueva imagen de disco</button>
        <button class="btn secondary" id="go-explore-existing">Explorar imagen existente</button>
        <button class="btn secondary" id="refresh-jobs">Ver trabajos guardados</button>
      </div>
      <div id="jobs-box" class="muted" style="margin-top:14px"></div>
    </section>
    ${
      priv
        ? `<section class="panel">
      <h2>${escapeHtml(priv.title)}</h2>
      <ol class="checklist">${priv.steps.map((s) => `<li>${escapeHtml(s)}</li>`).join("")}</ol>
      ${priv.notes.map((n) => `<p class="muted">${escapeHtml(n)}</p>`).join("")}
    </section>`
        : ""
    }
  `;
}

function cloneView(): string {
  const disks = state.disks
    .map((d) => {
      const selected = state.selectedSource?.id === d.id ? "selected" : "";
      const badge = d.isRemovable ? "extraíble" : d.isInternal ? "interno" : "disco";
      return `
        <button class="disk ${selected}" data-disk="${escapeHtml(d.id)}">
          <div>
            <div class="title">${escapeHtml(d.name)}</div>
            <div class="meta">${escapeHtml(d.path)} · ${fmt(d.sizeBytes)}${d.model ? " · " + escapeHtml(d.model) : ""}</div>
          </div>
          <span class="badge">${badge}</span>
        </button>`;
    })
    .join("");

  const v = state.validation;
  const validationHtml = v
    ? `<div class="validation ${v.ok ? "ok-box" : "warn-box"}">
        <p><strong>Resumen:</strong> ${escapeHtml(v.summary)}</p>
        ${v.warnings.map((w) => `<p class="warn-line">⚠ ${escapeHtml(w)}</p>`).join("")}
        ${v.errors.map((e) => `<p class="error-line">✗ ${escapeHtml(e)}</p>`).join("")}
      </div>`
    : "";

  const showInternal = state.selectedSource?.isInternal;
  const showForce = state.forceOverwrite;

  return `
    <section class="panel">
      <h2>Selecciona el disco de origen</h2>
      <p class="lead">Solo lectura del origen. No escribas en el disco dañado. Revisa privilegios en Inicio si fallan las lecturas.</p>
      <div class="row" style="margin-bottom:12px">
        <button class="btn secondary" id="reload-disks" ${state.busy ? "disabled" : ""}>Actualizar lista</button>
      </div>
      <div class="grid-disks">${disks || `<div class="empty">No se detectaron discos.</div>`}</div>
    </section>
    <section class="panel">
      <h2>Destino de la imagen</h2>
      <p class="lead">Elige un archivo .img en un disco sano con espacio suficiente (origen + margen).</p>
      <div class="row">
        <div class="field">
          <label>Ruta de imagen</label>
          <input id="image-path" value="${escapeHtml(state.imagePath)}" placeholder="/ruta/sana/disco.img" />
        </div>
        <button class="btn secondary" id="pick-image">Elegir…</button>
      </div>
      <label class="row" style="margin-top:12px">
        <input type="checkbox" id="force-overwrite" ${state.forceOverwrite ? "checked" : ""} />
        Forzar sobrescritura del destino (-f de ddrescue)
      </label>
      ${
        showInternal
          ? `<label class="row confirm-danger"><input type="checkbox" id="confirm-internal" ${state.confirmInternal ? "checked" : ""} /> Confirmo clonar un disco marcado como <strong>interno</strong></label>`
          : ""
      }
      ${
        showForce
          ? `<label class="row confirm-danger"><input type="checkbox" id="confirm-force" ${state.confirmForce ? "checked" : ""} /> Confirmo la <strong>sobrescritura forzada</strong> del destino</label>`
          : ""
      }
      ${validationHtml}
      <div class="actions">
        <button class="btn secondary" id="back-home">Atrás</button>
        <button class="btn secondary" id="validate-clone" ${!state.selectedSource || !state.imagePath || state.busy ? "disabled" : ""}>Validar</button>
        <button class="btn" id="start-clone" ${!state.selectedSource || !state.imagePath || state.busy ? "disabled" : ""}>
          Iniciar clonado
        </button>
      </div>
    </section>
  `;
}

function progressView(): string {
  const p = state.progress;
  const pct = p?.percent ?? 0;
  const stale = state.job?.lastError
    ? `<p class="warn-line">${escapeHtml(state.job.lastError)}</p>`
    : "";
  return `
    <section class="panel">
      <h2>Clonado en curso</h2>
      <p class="lead">${escapeHtml(state.job?.sourceName ?? "")} → ${escapeHtml(state.job?.imagePath ?? "")}</p>
      ${stale}
      <div class="progress-wrap">
        <div class="progress-bar"><span style="width:${pct.toFixed(2)}%"></span></div>
        <div class="muted" style="margin-top:8px">${escapeHtml(p?.message ?? "Iniciando…")} · ${pct.toFixed(2)}%</div>
      </div>
      <div class="stats">
        <div class="stat"><div class="k">Rescatado</div><div class="v">${fmt(p?.rescuedBytes ?? 0)}</div></div>
        <div class="stat"><div class="k">Errores</div><div class="v">${fmt(p?.errBytes ?? 0)}</div></div>
        <div class="stat"><div class="k">Pendiente</div><div class="v">${fmt(p?.nonTriedBytes ?? 0)}</div></div>
        <div class="stat"><div class="k">Estado</div><div class="v">${escapeHtml(state.job?.status ?? p?.status ?? "—")}</div></div>
      </div>
      <div class="actions">
        <button class="btn secondary" id="pause-job">Pausar</button>
        <button class="btn" id="resume-job">Reanudar</button>
        <button class="btn danger" id="cancel-job">Cancelar</button>
        <button class="btn secondary" id="go-explore">Explorar imagen</button>
        <button class="btn secondary" id="refresh-progress">Actualizar progreso</button>
      </div>
      <div class="log">${state.logs
        .slice(-80)
        .map((l) => escapeHtml(l))
        .join("\n")}</div>
    </section>
  `;
}

function exploreView(): string {
  const rows = [...(state.listing?.entries ?? []), ...state.carved]
    .filter((e) => !e.isDir)
    .slice(0, 2000)
    .map((e) => {
      const checked = state.selectedLocators.has(e.locator) ? "checked" : "";
      return `<tr>
        <td><input type="checkbox" data-loc="${escapeHtml(e.locator)}" ${checked} /></td>
        <td>${escapeHtml(e.path || e.name)}</td>
        <td>${fmt(e.size)}</td>
        <td>${escapeHtml(e.fsType)}${e.deleted ? " · borrado/carved" : ""}</td>
      </tr>`;
    })
    .join("");

  return `
    <section class="panel">
      <h2>Explorar recuperables</h2>
      <p class="lead">Imagen: ${escapeHtml(state.imagePath || state.job?.imagePath || "—")}
        · FS: ${escapeHtml(state.listing?.fsType ?? "—")}
        · <span class="muted">APFS/HFS+ no soportados en v1</span></p>
      <div class="row">
        <div class="field">
          <label>Ruta de imagen</label>
          <input id="explore-image" value="${escapeHtml(state.imagePath || state.job?.imagePath || "")}" />
        </div>
        <button class="btn secondary" id="pick-explore-image">Elegir…</button>
        <button class="btn" id="scan-fs" ${state.busy ? "disabled" : ""}>Listar FS</button>
        <button class="btn secondary" id="scan-carve" ${state.busy ? "disabled" : ""}>Carving</button>
      </div>
      <p class="muted" style="margin-top:10px">${escapeHtml(state.listing?.message ?? "")}</p>
      <div class="table-wrap" style="margin-top:12px">
        <table>
          <thead><tr><th></th><th>Ruta</th><th>Tamaño</th><th>Tipo</th></tr></thead>
          <tbody>${rows || `<tr><td colspan="4" class="empty">Sin entradas todavía.</td></tr>`}</tbody>
        </table>
      </div>
      <div class="actions">
        <button class="btn" id="export-selected" ${state.selectedLocators.size === 0 || state.busy ? "disabled" : ""}>
          Exportar seleccionados (${state.selectedLocators.size})
        </button>
        <button class="btn secondary" id="back-progress">Volver</button>
      </div>
    </section>
  `;
}

function aboutView(): string {
  const m = state.meta;
  return `
    <section class="panel">
      <h2>Acerca de Sectora</h2>
      <p class="lead">${escapeHtml(m?.name ?? "Sectora")} ${m?.version ? "v" + escapeHtml(m.version) : ""}</p>
      <ul class="about-list">
        <li>Licencia de la app: <strong>${escapeHtml(m?.license ?? "MIT")}</strong></li>
        <li>GNU ddrescue embebido: <strong>${escapeHtml(m?.ddrescueLicense ?? "GPLv2+")}</strong></li>
        <li>Sistemas de archivos: ${(m?.filesystems ?? []).map(escapeHtml).join(", ")}</li>
        <li>Fuera de v1: ${(m?.notSupported ?? []).map(escapeHtml).join(", ")}</li>
        <li>Log: <code>${escapeHtml(m?.logPath ?? "—")}</code></li>
      </ul>
      <p class="muted">Ver CHANGELOG.md y THIRD_PARTY_DDRESCUE.md en el repositorio. Instaladores unsigned: Gatekeeper/SmartScreen pueden avisar.</p>
      <div class="actions">
        <button class="btn secondary" id="back-home">Volver</button>
        <button class="btn secondary" id="show-log">Ver log</button>
      </div>
      <pre class="log" id="about-log" hidden></pre>
    </section>
  `;
}

function escapeHtml(s: string): string {
  return s
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function bindEvents() {
  $("#go-about")?.addEventListener("click", async () => {
    state.screen = "about";
    state.error = "";
    try {
      state.meta = await invoke<AppMeta>("app_meta");
    } catch (e) {
      state.error = await friendlyError(e);
    }
    render();
  });

  $("#go-clone")?.addEventListener("click", async () => {
    state.screen = "clone";
    state.error = "";
    state.validation = null;
    render();
    await loadDisks();
  });

  $("#go-explore-existing")?.addEventListener("click", () => {
    state.screen = "explore";
    state.error = "";
    render();
  });

  $("#refresh-jobs")?.addEventListener("click", async () => {
    try {
      const jobs = await invoke<ImageJob[]>("list_jobs");
      const box = $("#jobs-box");
      if (box) {
        if (!jobs.length) {
          box.textContent = "No hay trabajos guardados.";
        } else {
          box.innerHTML = jobs
            .slice(0, 8)
            .map(
              (j) =>
                `<div><button class="btn secondary job-open" data-id="${j.id}" style="margin:4px 0">${escapeHtml(j.sourceName)} · ${escapeHtml(j.status)}</button></div>`,
            )
            .join("");
          box.querySelectorAll(".job-open").forEach((btn) => {
            btn.addEventListener("click", async () => {
              const id = (btn as HTMLElement).dataset.id!;
              state.job = await invoke<ImageJob>("get_job", { jobId: id });
              state.imagePath = state.job.imagePath;
              state.screen = "progress";
              if (state.job.mapPath) {
                try {
                  state.progress = await invoke<Progress>("image_progress", {
                    jobId: state.job.id,
                    mapPath: state.job.mapPath,
                    sourceSize: state.job.sourceSize,
                  });
                } catch {
                  /* ignore */
                }
              }
              render();
            });
          });
        }
      }
    } catch (e) {
      state.error = await friendlyError(e);
      render();
    }
  });

  $("#reload-disks")?.addEventListener("click", () => loadDisks());
  $("#back-home")?.addEventListener("click", () => {
    state.screen = "home";
    render();
  });

  document.querySelectorAll("[data-disk]").forEach((el) => {
    el.addEventListener("click", () => {
      const id = (el as HTMLElement).dataset.disk;
      state.selectedSource = state.disks.find((d) => d.id === id) ?? null;
      state.confirmInternal = false;
      state.validation = null;
      render();
    });
  });

  $("#image-path")?.addEventListener("change", (e) => {
    state.imagePath = (e.target as HTMLInputElement).value;
  });
  $("#image-path")?.addEventListener("input", (e) => {
    state.imagePath = (e.target as HTMLInputElement).value;
  });
  $("#force-overwrite")?.addEventListener("change", (e) => {
    state.forceOverwrite = (e.target as HTMLInputElement).checked;
    state.confirmForce = false;
    state.validation = null;
    render();
  });
  $("#confirm-internal")?.addEventListener("change", (e) => {
    state.confirmInternal = (e.target as HTMLInputElement).checked;
  });
  $("#confirm-force")?.addEventListener("change", (e) => {
    state.confirmForce = (e.target as HTMLInputElement).checked;
  });

  $("#pick-image")?.addEventListener("click", async () => {
    const path = await save({
      title: "Guardar imagen del disco",
      defaultPath: "sectora-disk.img",
      filters: [{ name: "Imagen", extensions: ["img", "dd", "raw"] }],
    });
    if (typeof path === "string") {
      state.imagePath = path;
      render();
    }
  });

  $("#validate-clone")?.addEventListener("click", () => runValidate());
  $("#start-clone")?.addEventListener("click", () => startClone());
  $("#pause-job")?.addEventListener("click", async () => {
    if (!state.job) return;
    try {
      await invoke("pause_image", { jobId: state.job.id });
      state.job = await invoke("get_job", { jobId: state.job.id });
      render();
    } catch (e) {
      state.error = await friendlyError(e);
      render();
    }
  });
  $("#resume-job")?.addEventListener("click", async () => {
    if (!state.job) return;
    try {
      await invoke("resume_image", { jobId: state.job.id });
      state.job = await invoke("get_job", { jobId: state.job.id });
      render();
    } catch (e) {
      state.error = await friendlyError(e);
      render();
    }
  });
  $("#cancel-job")?.addEventListener("click", async () => {
    if (!state.job) return;
    try {
      await invoke("cancel_image", { jobId: state.job.id });
      state.job = await invoke("get_job", { jobId: state.job.id });
      render();
    } catch (e) {
      state.error = await friendlyError(e);
      render();
    }
  });
  $("#refresh-progress")?.addEventListener("click", async () => {
    if (!state.job) return;
    try {
      state.progress = await invoke<Progress>("image_progress", {
        jobId: state.job.id,
        mapPath: state.job.mapPath,
        sourceSize: state.job.sourceSize,
      });
      state.job = await invoke("get_job", { jobId: state.job.id });
      render();
    } catch (e) {
      state.error = await friendlyError(e);
      render();
    }
  });
  $("#go-explore")?.addEventListener("click", () => {
    state.imagePath = state.job?.imagePath || state.imagePath;
    state.screen = "explore";
    render();
  });
  $("#back-progress")?.addEventListener("click", () => {
    state.screen = state.job ? "progress" : "home";
    render();
  });

  $("#pick-explore-image")?.addEventListener("click", async () => {
    const path = await open({
      multiple: false,
      title: "Abrir imagen de disco",
      filters: [{ name: "Imagen", extensions: ["img", "dd", "raw", "bin"] }],
    });
    if (typeof path === "string") {
      state.imagePath = path;
      render();
    }
  });

  const syncExplorePath = () => {
    const el = $("#explore-image") as HTMLInputElement | null;
    if (el?.value) state.imagePath = el.value;
  };
  $("#explore-image")?.addEventListener("change", syncExplorePath);
  $("#explore-image")?.addEventListener("input", syncExplorePath);

  $("#scan-fs")?.addEventListener("click", async () => {
    syncExplorePath();
    const imagePath = state.imagePath || state.job?.imagePath;
    if (!imagePath) return;
    state.busy = true;
    state.error = "";
    render();
    try {
      state.listing = await invoke<Listing>("list_recoverable", { imagePath });
      state.imagePath = imagePath;
    } catch (e) {
      state.error = await friendlyError(e);
    } finally {
      state.busy = false;
      render();
    }
  });

  $("#scan-carve")?.addEventListener("click", async () => {
    syncExplorePath();
    const imagePath = state.imagePath || state.job?.imagePath;
    if (!imagePath) return;
    state.busy = true;
    state.error = "";
    render();
    try {
      state.carved = await invoke<RecoverableEntry[]>("carve_scan", {
        imagePath,
        maxHits: 150,
      });
      state.imagePath = imagePath;
    } catch (e) {
      state.error = await friendlyError(e);
    } finally {
      state.busy = false;
      render();
    }
  });

  document.querySelectorAll("[data-loc]").forEach((el) => {
    el.addEventListener("change", () => {
      const loc = (el as HTMLInputElement).dataset.loc!;
      if ((el as HTMLInputElement).checked) state.selectedLocators.add(loc);
      else state.selectedLocators.delete(loc);
      const btn = $("#export-selected") as HTMLButtonElement | null;
      if (btn) {
        btn.disabled = state.selectedLocators.size === 0 || state.busy;
        btn.textContent = `Exportar seleccionados (${state.selectedLocators.size})`;
      }
    });
  });

  $("#export-selected")?.addEventListener("click", async () => {
    const imagePath = state.imagePath || state.job?.imagePath;
    if (!imagePath || state.selectedLocators.size === 0) return;
    const destDir = await open({
      directory: true,
      title: "Carpeta destino para archivos recuperados",
    });
    if (typeof destDir !== "string") return;

    const all = [...(state.listing?.entries ?? []), ...state.carved];
    const items: [string, string][] = [];
    for (const loc of state.selectedLocators) {
      const entry = all.find((e) => e.locator === loc);
      const name = entry?.name || `file_${items.length}`;
      items.push([loc, `${destDir}/${name}`]);
    }
    state.busy = true;
    render();
    try {
      await invoke("export_files", { imagePath, items });
      state.error = "";
      alert(`Exportados ${items.length} archivo(s) a ${destDir}`);
    } catch (e) {
      state.error = await friendlyError(e);
    } finally {
      state.busy = false;
      render();
    }
  });

  $("#show-log")?.addEventListener("click", async () => {
    const pre = $("#about-log") as HTMLPreElement | null;
    if (!pre) return;
    try {
      const text = await invoke<string>("read_app_log", { maxBytes: 24_000 });
      pre.hidden = false;
      pre.textContent = text || "(log vacío)";
    } catch (e) {
      state.error = await friendlyError(e);
      render();
    }
  });
}

async function loadDisks() {
  state.busy = true;
  state.error = "";
  render();
  try {
    state.disks = await invoke<DiskInfo[]>("list_disks");
  } catch (e) {
    state.error = await friendlyError(e);
  } finally {
    state.busy = false;
    render();
  }
}

async function runValidate(): Promise<CloneValidation | null> {
  if (!state.selectedSource || !state.imagePath) return null;
  try {
    state.validation = await invoke<CloneValidation>("validate_clone_plan", {
      source: state.selectedSource,
      imagePath: state.imagePath,
      forceOverwrite: state.forceOverwrite,
      confirmInternal: state.confirmInternal,
      confirmForce: state.confirmForce,
    });
    render();
    return state.validation;
  } catch (e) {
    state.error = await friendlyError(e);
    render();
    return null;
  }
}

async function startClone() {
  if (!state.selectedSource || !state.imagePath) return;
  const validation = await runValidate();
  if (!validation?.ok) {
    state.error =
      validation?.errors.join(" ") ||
      "La validación del clonado falló. Revisa avisos y confirmaciones.";
    render();
    return;
  }

  const ok = await ask(
    `${validation.summary}\n\n¿Iniciar clonado con ddrescue?\nNo escribas en el disco de origen.`,
    { title: "Confirmar clonado", kind: "warning" },
  );
  if (!ok) return;

  state.busy = true;
  state.error = "";
  render();
  try {
    state.job = await invoke<ImageJob>("create_image_job", {
      sourcePath: state.selectedSource.path,
      imagePath: state.imagePath,
      sourceName: state.selectedSource.name,
      sourceSize: state.selectedSource.sizeBytes,
      forceOverwrite: state.forceOverwrite,
    });
    await invoke("start_image", { jobId: state.job.id });
    state.screen = "progress";
    state.logs = ["ddrescue iniciado…"];
  } catch (e) {
    state.error = await friendlyError(e);
  } finally {
    state.busy = false;
    render();
  }
}

async function setupListeners() {
  await listen<Progress>("ddrescue-progress", (ev) => {
    state.progress = ev.payload;
    if (state.screen === "progress") {
      const bar = document.querySelector(".progress-bar > span") as HTMLElement | null;
      if (bar) bar.style.width = `${ev.payload.percent.toFixed(2)}%`;
      const stats = document.querySelectorAll(".stat .v");
      if (stats.length >= 4) {
        stats[0].textContent = fmt(ev.payload.rescuedBytes);
        stats[1].textContent = fmt(ev.payload.errBytes);
        stats[2].textContent = fmt(ev.payload.nonTriedBytes);
      }
    }
  });
  await listen<{ jobId: string; line: string }>("ddrescue-log", (ev) => {
    state.logs.push(ev.payload.line);
    if (state.logs.length > 200) state.logs.shift();
    const log = document.querySelector(".log");
    if (log) {
      log.textContent = state.logs.slice(-80).join("\n");
      log.scrollTop = log.scrollHeight;
    }
  });
  await listen<{ jobId: string; ok: boolean }>("ddrescue-finished", async (ev) => {
    if (state.job?.id === ev.payload.jobId) {
      state.job = await invoke("get_job", { jobId: ev.payload.jobId });
      state.logs.push(ev.payload.ok ? "Clonado finalizado." : "Clonado terminó con error.");
      render();
    }
  });
}

window.addEventListener("DOMContentLoaded", async () => {
  try {
    state.meta = await invoke<AppMeta>("app_meta");
    state.privileges = await invoke<PrivilegeChecklist>("privilege_checklist");
  } catch {
    /* browser preview */
  }
  render();
  try {
    await setupListeners();
  } catch {
    /* browser preview without tauri */
  }
});
