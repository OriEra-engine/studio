/** 用户可见错误的人话化（2026-10-26 三轮测评 P0：原始异常串直甩=天书）。
 *  映射常见故障为「发生了什么 + 下一步」，原始错误折叠在括号里留给排查。 */
export function humanizeError(e: unknown): string {
  const raw = String(e);
  const compact = raw.length > 120 ? raw.slice(0, 120) + "…" : raw;
  if (/401|403|unauthorized|invalid api key|authentication|AuthenticationError/i.test(raw))
    return `API Key 无效或已过期——请到「模型中心」重新粘贴并保存 Key。（${compact}）`;
  if (/429|rate.?limit|quota|Throttling/i.test(raw))
    return "请求太频繁或额度用完——等一两分钟再试，或到供应商控制台确认额度。";
  if (/timeout|timed out|network|fetch|Failed to fetch|ECONN|error sending request|connection/i.test(raw))
    return `网络不通或超时——检查网络后重试；配了代理的确认代理在跑。（${compact}）`;
  if (/no api key|尚未保存.*密钥|未配置|not configured/i.test(raw))
    return "还没配置凭证——到「模型中心」或「AI 素材中心」粘贴并保存对应的 API Key。";
  return `操作失败，请重试；反复失败请到「模型中心」测试连接。（${compact}）`;
}
