"use client"; // Next.jsで状態管理を使うためのおまじない

import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

// Rust側で定義したデータの形をTypeScriptでも定義
type WindowInfo = {
  id: number;
  title: string;
  app_name: string;
};
import styles from "./page.module.css";

export default function Home() {
  const [windows, setWindows] = useState<WindowInfo[]>([]);
  useEffect(() => {
    // Rustの 'get_windows' コマンドを呼び出す
    invoke<WindowInfo[]>("get_windows")
      .then((data) => {
        setWindows(data);
      })
      .catch((err) => {
        console.error("ウィンドウ取得エラー:", err);
      });
  }, []);
  return (
    <div className={styles.page}>
      <main className={styles.main}>
        <h1>Screen Recorder</h1>
        <button type="button">Record</button>
        <p>ウィンドウ一覧</p>
        <ul>
          {windows.map((win) => (
            <li key={win.id}>
              {win.app_name} - {win.title}
            </li>
          ))}
        </ul>
      </main>
    </div>
  );
}
