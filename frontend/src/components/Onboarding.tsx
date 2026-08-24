/**
 * 首次启动聚光灯引导（PRD §6.5.6 导入引导，driver.js）。
 * - 仅首次启动触发：localStorage `aidms-onboarding-done` 标记，非首次跳过；
 * - 步骤指向：侧栏导航项（资料库/搜索/问答）、资料库「导入 / 新建」按钮、主体切换器；
 * - driver.css 随 Vite 打包为同源样式表（import 方式），避免放宽 CSP；
 * - 缺失元素自动跳过（如不在资料库页时无导入按钮），保证任何入口都不会报错卡死。
 */
import { useEffect } from "react";
import { driver } from "driver.js";
import "driver.js/dist/driver.css";

const DONE_KEY = "aidms-onboarding-done";

interface StepDef {
  element: string;
  popover: { title: string; description: string; side?: "top" | "right" | "bottom" | "left" };
}

const STEPS: StepDef[] = [
  {
    element: "#nav-library",
    popover: {
      title: "资料库",
      description: "这里集中管理所有企业资料：文件与业务条目，可按主体 / 类型 / 标签筛选。",
      side: "right",
    },
  },
  {
    element: "#library-import-btn",
    popover: {
      title: "导入 / 新建",
      description: "拖入文件或新建业务条目，选择归属主体后自动入库、建索引、可搜索。",
      side: "bottom",
    },
  },
  {
    element: "#entity-switcher",
    popover: {
      title: "主体切换器",
      description: "按公司主体一键过滤资料；更细的组合可在资料库「筛选」与「高级筛选」中完成。",
      side: "bottom",
    },
  },
  {
    element: "#nav-search",
    popover: {
      title: "搜索",
      description: "关键词 / 语义 / 融合三种模式检索全部资料，命中片段高亮展示。",
      side: "right",
    },
  },
  {
    element: "#nav-qa",
    popover: {
      title: "问答",
      description: "基于资料库内容提问，AI 引用原文作答，可追溯来源。",
      side: "right",
    },
  },
];

export function Onboarding() {
  useEffect(() => {
    // 非首次启动直接跳过
    if (localStorage.getItem(DONE_KEY)) return;

    // 等首屏渲染完成、DOM 就绪后再启动聚光灯
    const timer = window.setTimeout(() => {
      // 过滤掉当前不存在的元素（例如不在资料库首页时无导入按钮）
      const steps = STEPS.filter((s) => document.querySelector(s.element));
      if (steps.length === 0) {
        localStorage.setItem(DONE_KEY, "1");
        return;
      }
      const d = driver({
        showProgress: true,
        steps,
        onDestroyed: () => {
          localStorage.setItem(DONE_KEY, "1");
        },
      });
      d.drive();
    }, 400);

    return () => window.clearTimeout(timer);
  }, []);

  return null;
}
