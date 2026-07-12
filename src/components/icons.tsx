// 模块图标 — 使用 Lucide React 专业图标库
import React from 'react';
import {
  Clapperboard,
  Mic,
  Sparkles,
  Settings as SettingsIcon,
  Moon,
  Sun,
  PanelRight,
  PanelRightOpen,
  Menu,
  X,
  Download,
  ChevronRight,
  Plus,
  ArrowLeft,
  Upload,
  MoveUp,
  MoveDown,
  Pencil,
  Trash2,
} from 'lucide-react';

interface IconProps {
  size?: number;
}

export const FilmIcon = ({ size = 18 }: IconProps) => (
  <Clapperboard size={size} strokeWidth={1.6} />
);

export const MicIcon = ({ size = 18 }: IconProps) => (
  <Mic size={size} strokeWidth={1.6} />
);

export const SparkIcon = ({ size = 18 }: IconProps) => (
  <Sparkles size={size} strokeWidth={1.6} />
);

export const GearIcon = ({ size = 18 }: IconProps) => (
  <SettingsIcon size={size} strokeWidth={1.6} />
);

export const MoonIcon = ({ size = 16 }: IconProps) => (
  <Moon size={size} strokeWidth={1.6} />
);

export const SunIcon = ({ size = 16 }: IconProps) => (
  <Sun size={size} strokeWidth={1.6} />
);

export const InspectorIcon = ({ size = 16 }: IconProps) => (
  <PanelRight size={size} strokeWidth={1.6} />
);

export const InspectorOpenIcon = ({ size = 16 }: IconProps) => (
  <PanelRightOpen size={size} strokeWidth={1.6} />
);

export const MenuIcon = ({ size = 16 }: IconProps) => (
  <Menu size={size} strokeWidth={1.6} />
);

export const CloseIcon = ({ size = 16 }: IconProps) => (
  <X size={size} strokeWidth={1.6} />
);

export const DownloadIcon = ({ size = 14 }: IconProps) => (
  <Download size={size} strokeWidth={1.8} />
);

export const ChevronRightIcon = ({ size = 14 }: IconProps) => (
  <ChevronRight size={size} strokeWidth={1.8} />
);

export const PlusIcon = ({ size = 14 }: IconProps) => (
  <Plus size={size} strokeWidth={1.8} />
);

export const ArrowLeftIcon = ({ size = 14 }: IconProps) => (
  <ArrowLeft size={size} strokeWidth={1.8} />
);

export const UploadIcon = ({ size = 14 }: IconProps) => (
  <Upload size={size} strokeWidth={1.8} />
);

export const MoveUpIcon = ({ size = 14 }: IconProps) => (
  <MoveUp size={size} strokeWidth={1.8} />
);

export const MoveDownIcon = ({ size = 14 }: IconProps) => (
  <MoveDown size={size} strokeWidth={1.8} />
);

export const PencilIcon = ({ size = 14 }: IconProps) => (
  <Pencil size={size} strokeWidth={1.8} />
);

export const Trash2Icon = ({ size = 14 }: IconProps) => (
  <Trash2 size={size} strokeWidth={1.8} />
);