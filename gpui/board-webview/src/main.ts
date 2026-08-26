/**
 * main.ts — GPUI 宿主 Wry webview 的白板入口（P0 检查点④）。
 *
 * 复用 app/src/lib 的 tldraw-runtime.ts / tldraw-board.ts / tldraw-markdown.ts
 * （逐字拷贝，未改动）。本文件只做两件事：
 *   1. 把 runtime 的 mountBoard 挂到固定 DOM 节点上；
 *   2. 与 GPUI 宿主之间的 IPC 协议胶水：
 *      - board→host：window.ipc.postMessage(JSON 字符串)
 *        { type: 'solomd-boot' }                          页面就绪
 *        { type: 'solomd-loaded', bytes }                 初始快照已载入画板
 *        { type: 'solomd-snapshot', body }                用户编辑后的新快照（防抖）
 *      - host→board：evaluate_script 调 window.solomdBoard.loadSnapshot(json)
 *
 * 快照回写闭环（自动化冒烟）：boot → host 下发初始快照 → 画板载入 → 1s 后
 * board 主动回报当前 store 快照（solomd-snapshot）→ 宿主重写围栏体并打日志。
 */

import { mountBoard, type BoardHandle } from './tldraw-runtime';
import type { BoardThemeTokens } from './tldraw-board';

declare global {
  interface Window {
    /** wry 注入的桥；类型按 wry 的实际形态最小声明。 */
    ipc?: { postMessage(msg: string): void };
    solomdBoard: {
      loadSnapshot(json: string): void;
      ready: boolean;
    };
  }
}

const THEME: BoardThemeTokens = { colorScheme: 'light', locale: 'zh' };

function post(msg: unknown): void {
  window.ipc?.postMessage(JSON.stringify(msg));
}

let handle: BoardHandle | null = null;

async function boot(): Promise<void> {
  const el = document.getElementById('board');
  if (!el) return;

  try {
    handle = await mountBoard(el, {
      // 初始为空板；宿主在收到 solomd-boot 后会 evaluate_script 下发围栏快照。
      snapshot: '{}',
      theme: THEME,
      debounceMs: 200,
      onSnapshotChange: (json) => {
        post({ type: 'solomd-snapshot', body: json });
        setStatus(`快照已回报：${json.length} bytes`);
      },
    });

    window.solomdBoard.ready = true;

    window.solomdBoard.loadSnapshot = (json: string) => {
      handle?.loadSnapshot(json);
      post({ type: 'solomd-loaded', bytes: json.length });
      setStatus(`已载入宿主下发的快照：${json.length} bytes`);

      // 自动化闭环：载入完成后稍候，把 store 当前内容主动回报一次，
      // 让宿主端无需人工编辑即可验证「围栏 → 画板 → 回写」全链路。
      setTimeout(() => {
        const body = handle?.getSnapshotString() ?? '';
        post({ type: 'solomd-snapshot', body });
      }, 1000);
    };

    post({ type: 'solomd-boot' });
    setStatus('画板已挂载，等待宿主下发快照…');
  } catch (err) {
    setStatus(`挂载失败：${String(err)}`);
    post({ type: 'solomd-boot-error', error: String(err) });
  }
}

function setStatus(text: string): void {
  const el = document.getElementById('status');
  if (el) el.textContent = text;
}

window.solomdBoard = { loadSnapshot: () => {}, ready: false };

// 兼容无 ipc 桥的裸浏览器调试场景。
if (!window.ipc) {
  setStatus('(未检测到 window.ipc —— 非宿主环境，仅作本地调试)');
}

void boot();
