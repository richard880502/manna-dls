// Files picked or dropped for a bulk upload, each with the path the browser gave it: a folder
// picked with webkitdirectory or dropped onto the page keeps its relative path, so the folders'
// names can fill in the category and tags too. A plain multi-file pick has just the file name.

export interface PickedFile {
  file: File;
  /** "學測英文/114/114學測英文_試題.docx", or just the file name. */
  path: string;
}

export function fromFileList(list: FileList | readonly File[] | null): PickedFile[] {
  if (!list) return [];
  return Array.from(list).map((file) => ({ file, path: file.webkitRelativePath || file.name }));
}

function readFile(entry: FileSystemFileEntry): Promise<File> {
  return new Promise((resolve, reject) => entry.file(resolve, reject));
}

/** Every entry of a folder: readEntries answers in batches until it answers with none. */
async function readAll(dir: FileSystemDirectoryEntry): Promise<FileSystemEntry[]> {
  const reader = dir.createReader();
  const all: FileSystemEntry[] = [];
  for (;;) {
    const batch = await new Promise<FileSystemEntry[]>((resolve, reject) => reader.readEntries(resolve, reject));
    if (batch.length === 0) return all;
    all.push(...batch);
  }
}

async function walk(entry: FileSystemEntry, out: PickedFile[]): Promise<void> {
  if (entry.isFile) {
    const file = await readFile(entry as FileSystemFileEntry);
    out.push({ file, path: entry.fullPath.replace(/^\/+/, "") || file.name });
  } else if (entry.isDirectory) {
    for (const child of await readAll(entry as FileSystemDirectoryEntry)) await walk(child, out);
  }
}

/**
 * The entries of a drop. Must be called while the drop event is being handled: the browser
 * empties `items` afterwards. Null when the browser does not give entries (then use `files`).
 */
export function dropEntries(data: DataTransfer): FileSystemEntry[] | null {
  const items = Array.from(data.items ?? []).filter((i) => i.kind === "file");
  if (items.length === 0 || items.some((i) => typeof i.webkitGetAsEntry !== "function")) return null;
  const entries = items.map((i) => i.webkitGetAsEntry());
  return entries.every((e): e is FileSystemEntry => e !== null) ? entries : null;
}

/** The files of a drop, folders walked; falls back to the plain file list. */
export async function filesFromDrop(entries: FileSystemEntry[] | null, files: FileList | null): Promise<PickedFile[]> {
  if (!entries) return fromFileList(files);
  const out: PickedFile[] = [];
  for (const entry of entries) await walk(entry, out);
  return out;
}
