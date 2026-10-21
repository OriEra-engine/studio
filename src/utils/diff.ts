// 字符级 diff（中文文本按字比对比词级更准）——AI 重写确认制的对比高亮。
// 节拍文案都是短文本（<500 字），O(n·m) LCS 足够。

export interface Seg {
  text: string;
  kind: "same" | "add" | "del";
}

interface DiffSide {
  old: Seg[];
  new: Seg[];
}

export function charDiff(a: string, b: string): DiffSide {
  const x = [...a];
  const y = [...b];
  const n = x.length;
  const m = y.length;
  // LCS 长度表
  const dp: Uint32Array[] = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = x[i] === y[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  // 回溯出分段
  const oldSegs: Seg[] = [];
  const newSegs: Seg[] = [];
  const push = (arr: Seg[], kind: Seg["kind"], text: string) => {
    const last = arr[arr.length - 1];
    if (last && last.kind === kind) last.text += text;
    else arr.push({ kind, text });
  };
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (x[i] === y[j]) {
      push(oldSegs, "same", x[i]);
      push(newSegs, "same", y[j]);
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      push(oldSegs, "del", x[i]);
      i++;
    } else {
      push(newSegs, "add", y[j]);
      j++;
    }
  }
  while (i < n) push(oldSegs, "del", x[i++]);
  while (j < m) push(newSegs, "add", y[j++]);
  return { old: oldSegs, new: newSegs };
}
