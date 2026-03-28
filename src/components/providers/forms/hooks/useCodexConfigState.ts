import { useState, useCallback, useEffect, useRef } from "react";
import {
  extractCodexBaseUrl,
  setCodexBaseUrl as setCodexBaseUrlInConfig,
  extractCodexModelName,
  setCodexModelName as setCodexModelNameInConfig,
} from "@/utils/providerConfigUtils";
import { normalizeTomlText } from "@/utils/textNormalization";

interface UseCodexConfigStateProps {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
}

/**
 * 管理 Codex 配置状态
 * Codex 配置包含两部分：auth.json (JSON) 和 config.toml (TOML 字符串)
 */
export function useCodexConfigState({ initialData }: UseCodexConfigStateProps) {
  const [codexAuth, setCodexAuthState] = useState("");
  const [codexConfig, setCodexConfigState] = useState("");
  const [codexApiKey, setCodexApiKey] = useState("");
  const [codexBaseUrl, setCodexBaseUrl] = useState("");
  const [codexModelName, setCodexModelName] = useState("");
  const [codexAuthError, setCodexAuthError] = useState("");
  const [codexOAuth, setCodexOAuthState] = useState<Record<string, unknown> | null>(null);
  const [codexAuthMode, setCodexAuthModeState] = useState<"manual" | "oauth">(
    "manual",
  );

  const isUpdatingCodexBaseUrlRef = useRef(false);
  const isUpdatingCodexModelNameRef = useRef(false);

  // 初始化 Codex 配置（编辑模式）
  useEffect(() => {
    if (!initialData) return;

    const config = initialData.settingsConfig;
    if (typeof config === "object" && config !== null) {
      // 设置 auth.json
      const auth = (config as any).auth || {};
      setCodexAuthState(JSON.stringify(auth, null, 2));
      const oauth =
        typeof (config as any).oauth === "object" && (config as any).oauth !== null
          ? ((config as any).oauth as Record<string, unknown>)
          : null;
      setCodexOAuthState(oauth);
      setCodexAuthModeState(oauth ? "oauth" : "manual");

      // 设置 config.toml
      const configStr =
        typeof (config as any).config === "string"
          ? (config as any).config
          : "";
      setCodexConfigState(configStr);

      // 提取 Base URL
      const initialBaseUrl = extractCodexBaseUrl(configStr);
      if (initialBaseUrl) {
        setCodexBaseUrl(initialBaseUrl);
      }

      // 提取 Model Name
      const initialModelName = extractCodexModelName(configStr);
      if (initialModelName) {
        setCodexModelName(initialModelName);
      }

      // 提取 API Key
      try {
        if (auth && typeof auth.OPENAI_API_KEY === "string") {
          setCodexApiKey(auth.OPENAI_API_KEY);
        }
      } catch {
        // ignore
      }
    }
  }, [initialData]);

  // 与 TOML 配置保持基础 URL 同步
  useEffect(() => {
    if (isUpdatingCodexBaseUrlRef.current) {
      return;
    }
    const extracted = extractCodexBaseUrl(codexConfig) || "";
    setCodexBaseUrl((prev) => (prev === extracted ? prev : extracted));
  }, [codexConfig]);

  // 与 TOML 配置保持模型名称同步
  useEffect(() => {
    if (isUpdatingCodexModelNameRef.current) {
      return;
    }
    const extracted = extractCodexModelName(codexConfig) || "";
    setCodexModelName((prev) => (prev === extracted ? prev : extracted));
  }, [codexConfig]);

  // 获取 API Key（从 auth JSON）
  const getCodexAuthApiKey = useCallback((authString: string): string => {
    try {
      const auth = JSON.parse(authString || "{}");
      return typeof auth.OPENAI_API_KEY === "string" ? auth.OPENAI_API_KEY : "";
    } catch {
      return "";
    }
  }, []);

  // 从 codexAuth 中提取并同步 API Key
  useEffect(() => {
    const extractedKey = getCodexAuthApiKey(codexAuth);
    if (extractedKey !== codexApiKey) {
      setCodexApiKey(extractedKey);
    }
  }, [codexAuth, codexApiKey]);

  // 验证 Codex Auth JSON
  const validateCodexAuth = useCallback((value: string): string => {
    if (!value.trim()) return "";
    try {
      const parsed = JSON.parse(value);
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
        return "Auth JSON must be an object";
      }
      return "";
    } catch {
      return "Invalid JSON format";
    }
  }, []);

  // 设置 auth 并验证
  const setCodexAuth = useCallback(
    (value: string) => {
      setCodexAuthState(value);
      setCodexAuthError(validateCodexAuth(value));
    },
    [validateCodexAuth],
  );

  // 设置 config (支持函数更新)
  const setCodexConfig = useCallback(
    (value: string | ((prev: string) => string)) => {
      setCodexConfigState((prev) =>
        typeof value === "function"
          ? (value as (input: string) => string)(prev)
          : value,
      );
    },
    [],
  );

  // 处理 Codex API Key 输入并写回 auth.json
  const handleCodexApiKeyChange = useCallback(
    (key: string) => {
      const trimmed = key.trim();
      setCodexApiKey(trimmed);
      try {
        const auth = JSON.parse(codexAuth || "{}");
        auth.OPENAI_API_KEY = trimmed;
        setCodexAuth(JSON.stringify(auth, null, 2));
      } catch {
        // ignore
      }
    },
    [codexAuth, setCodexAuth],
  );

  const getCodexSettingsObject = useCallback((): Record<string, unknown> => {
    let auth: Record<string, unknown> = {};
    try {
      const parsed = JSON.parse(codexAuth || "{}");
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        auth = parsed as Record<string, unknown>;
      }
    } catch {
      auth = {};
    }

    return {
      auth,
      config: codexConfig ?? "",
      ...(codexOAuth ? { oauth: codexOAuth } : {}),
    };
  }, [codexAuth, codexConfig, codexOAuth]);

  const applyCodexSettingsPatch = useCallback(
    (patch: Record<string, unknown>) => {
      if (patch.auth && typeof patch.auth === "object" && !Array.isArray(patch.auth)) {
        setCodexAuth(JSON.stringify(patch.auth, null, 2));
      }
      if (typeof patch.config === "string") {
        setCodexConfig(patch.config);
      }
      const oauth =
        patch.oauth && typeof patch.oauth === "object" && !Array.isArray(patch.oauth)
          ? (patch.oauth as Record<string, unknown>)
          : null;
      setCodexOAuthState(oauth);
      setCodexAuthModeState(oauth ? "oauth" : "manual");
    },
    [setCodexAuth, setCodexConfig],
  );

  const setCodexAuthMode = useCallback((mode: "manual" | "oauth") => {
    setCodexAuthModeState(mode);
    if (mode === "manual") {
      setCodexOAuthState(null);
    }
  }, []);

  // 处理 Codex Base URL 变化
  const handleCodexBaseUrlChange = useCallback(
    (url: string) => {
      const sanitized = url.trim();
      setCodexBaseUrl(sanitized);

      isUpdatingCodexBaseUrlRef.current = true;
      setCodexConfig((prev) => setCodexBaseUrlInConfig(prev, sanitized));
      setTimeout(() => {
        isUpdatingCodexBaseUrlRef.current = false;
      }, 0);
    },
    [setCodexConfig],
  );

  // 处理 Codex Model Name 变化
  const handleCodexModelNameChange = useCallback(
    (modelName: string) => {
      const trimmed = modelName.trim();
      setCodexModelName(trimmed);

      isUpdatingCodexModelNameRef.current = true;
      setCodexConfig((prev) => setCodexModelNameInConfig(prev, trimmed));
      setTimeout(() => {
        isUpdatingCodexModelNameRef.current = false;
      }, 0);
    },
    [setCodexConfig],
  );

  // 处理 config 变化（同步 Base URL 和 Model Name）
  const handleCodexConfigChange = useCallback(
    (value: string) => {
      // 归一化中文/全角/弯引号，避免 TOML 解析报错
      const normalized = normalizeTomlText(value);
      setCodexConfig(normalized);

      if (!isUpdatingCodexBaseUrlRef.current) {
        const extracted = extractCodexBaseUrl(normalized) || "";
        if (extracted !== codexBaseUrl) {
          setCodexBaseUrl(extracted);
        }
      }

      if (!isUpdatingCodexModelNameRef.current) {
        const extractedModel = extractCodexModelName(normalized) || "";
        if (extractedModel !== codexModelName) {
          setCodexModelName(extractedModel);
        }
      }
    },
    [setCodexConfig, codexBaseUrl, codexModelName],
  );

  // 重置配置（用于预设切换）
  const resetCodexConfig = useCallback(
    (
      auth: Record<string, unknown>,
      config: string,
      oauth?: Record<string, unknown> | null,
    ) => {
      const authString = JSON.stringify(auth, null, 2);
      setCodexAuth(authString);
      setCodexConfig(config);
      setCodexOAuthState(oauth ?? null);
      setCodexAuthModeState(oauth ? "oauth" : "manual");

      const baseUrl = extractCodexBaseUrl(config);
      if (baseUrl) {
        setCodexBaseUrl(baseUrl);
      }

      const modelName = extractCodexModelName(config);
      if (modelName) {
        setCodexModelName(modelName);
      } else {
        setCodexModelName("");
      }

      // 提取 API Key
      try {
        if (auth && typeof auth.OPENAI_API_KEY === "string") {
          setCodexApiKey(auth.OPENAI_API_KEY);
        } else {
          setCodexApiKey("");
        }
      } catch {
        setCodexApiKey("");
      }
    },
    [setCodexAuth, setCodexConfig],
  );

  return {
    codexAuth,
    codexConfig,
    codexApiKey,
    codexBaseUrl,
    codexModelName,
    codexAuthError,
    codexOAuth,
    codexAuthMode,
    setCodexAuth,
    setCodexConfig,
    setCodexAuthMode,
    handleCodexApiKeyChange,
    handleCodexBaseUrlChange,
    handleCodexModelNameChange,
    handleCodexConfigChange,
    resetCodexConfig,
    applyCodexSettingsPatch,
    getCodexSettingsObject,
    getCodexAuthApiKey,
    validateCodexAuth,
  };
}
