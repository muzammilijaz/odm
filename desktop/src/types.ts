export type TaskStatus = "Queued" | "Downloading" | "Paused" | "Completed" | "Failed" | "Cancelled";

export interface Task {
  playlist_group: string | null;
  playlist_title: string | null;
  id: number;
  url: string;
  dest_path: string;
  category: string | null;
  status: TaskStatus;
  total_bytes: number | null;
  downloaded_bytes: number;
  created_at: string;
  completed_at: string | null;
  retry_count: number;
  error_message: string | null;
  allow_playlist: boolean;
  video_quality: number | null;
  actual_video_quality: number | null;
  title: string | null;
  thumbnail_url: string | null;
}

/** Task plus a client-computed instantaneous speed (the backend doesn't
 * persist speed, only byte counts — see useTaskSpeeds). */
export interface TaskWithSpeed extends Task {
  bytesPerSec: number;
}

export interface Category {
  name: string;
  default_folder: string;
  extensions: string[];
}
