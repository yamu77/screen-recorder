"use client"; // Next.jsで状態管理を使うためのおまじない

// biome-ignore assist/source/organizeImports: <explanation>
import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import Button from "@/components/button";

// Rust側で定義したデータの形をTypeScriptでも定義
type WindowInfo = {
  id: number;
  title: string;
  app_name: string;
};
import styles from "./page.module.css";
import {
  Select,
  SelectMenu,
  SelectOption,
  SelectTrigger,
} from "@/components/select";

const handleCapture = async (title: string) => {
  try {
    // コマンド名と渡す引数を修正
    const result = await invoke<string>("capture_selected_window", {
      title: title,
    });
    console.log(result);
    alert(result);
  } catch (err) {
    console.error("キャプチャ失敗:", err);
    alert("失敗しちゃったみたい…");
  }
};

export default function Home() {
  const [windows, setWindows] = useState<WindowInfo[]>([]);
  const [selectedWindowId, setSelectedWindowId] = useState("");
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
        <div className={styles.buttonWrapper}>
          <Button
            label="スクリーンショット"
            variant="fill"
            size="lg"
            color="primary"
            startIcon="video"
            endIcon="video"
            onClick={() => {
              if (!selectedWindowId) {
                alert("先にウィンドウを選んで。");
                return;
              }
              // 選択されているID（selectedWindowId）と一致するウィンドウを、リストから探す
              const targetWindow = windows.find(
                (w) => String(w.id) === selectedWindowId,
              );
              // もし見つかったら、そのタイトルを渡してキャプチャを実行する
              if (targetWindow) {
                handleCapture(targetWindow.title);
              } else {
                alert("選択されたウィンドウが見つからないみたい…");
              }
            }}
          />
          <Button
            label="録画開始"
            variant="fill"
            size="lg"
            color="primary"
            startIcon="video"
            endIcon="video"
          />
        </div>
        <Select
          options={windows.map((win) => ({
            label: win.title,
            value: String(win.id),
          }))}
          value={selectedWindowId}
          onChange={(event) => setSelectedWindowId(event.target.value)}
          name="window"
        >
          <SelectTrigger>
            <Button
              label={
                windows.find((win) => String(win.id) === selectedWindowId)
                  ?.title ?? "ウィンドウを選択"
              }
              variant="outline"
              size="md"
              color="primary"
            />
          </SelectTrigger>
          <SelectMenu>
            {windows.map((win) => (
              <SelectOption key={win.id} value={String(win.id)}>
                {win.title}
              </SelectOption>
            ))}
          </SelectMenu>
        </Select>
      </main>
    </div>
  );
}
