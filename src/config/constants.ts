// Provider 类型常量
export const PROVIDER_TYPES = {
  GITHUB_COPILOT: "github_copilot",
} as const;

// 用量脚本模板类型常量
export const TEMPLATE_TYPES = {
  CUSTOM: "custom",
  GENERAL: "general",
  NEW_API: "newapi",
  CODEX_CHATGPT_OAUTH: "codex_chatgpt_oauth",
  GITHUB_COPILOT: "github_copilot",
} as const;

export type TemplateType =
  (typeof TEMPLATE_TYPES)[keyof typeof TEMPLATE_TYPES];
