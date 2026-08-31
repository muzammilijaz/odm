import { invoke } from "@tauri-apps/api/core";
import type { Category, Task } from "./types";

export const api = {
  addDownload: (url: string, filename?: string, playlist?: boolean) =>
    invoke<Task>("add_download", { url, filename: filename || null, playlist: playlist ?? false }),
  listDownloads: () => invoke<Task[]>("list_downloads"),
  pauseDownload: (id: number) => invoke<void>("pause_download", { id }),
  resumeDownload: (id: number) => invoke<void>("resume_download", { id }),
  cancelDownload: (id: number) => invoke<void>("cancel_download", { id }),
  deleteDownload: (id: number, deleteFile: boolean) => invoke<void>("delete_download", { id, deleteFile }),
  renameDownload: (id: number, newPath: string) => invoke<void>("rename_download", { id, newPath }),
  openWithDialog: (path: string) => invoke<void>("open_with_dialog", { path }),
  listCategories: () => invoke<Category[]>("list_categories"),
  addCategory: (name: string, defaultFolder: string) => invoke<void>("add_category", { name, defaultFolder }),
  removeCategory: (name: string) => invoke<void>("remove_category", { name }),
  addCategoryExtension: (category: string, extension: string) => invoke<void>("add_category_extension", { category, extension }),
  removeCategoryExtension: (category: string, extension: string) => invoke<void>("remove_category_extension", { category, extension }),
  updateYtdlp: () => invoke<string>("update_ytdlp"),
  getSetting: (key: string) => invoke<string | null>("get_setting", { key }),
  setSetting: (key: string, value: string) => invoke<void>("set_setting", { key, value }),
  importCookiesFile: (path: string) => invoke<string>("import_cookies_file", { path }),
  clearCookiesFile: () => invoke<void>("clear_cookies_file"),
};
