// 统一图标（无外部图标库，inline SVG，strokeWidth 一致）。
type P = { size?: number };
const s = (n = 14) => ({ width: n, height: n, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 2 });

export const IconSearch = ({ size }: P) => (<svg {...s(size)}><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></svg>);
export const IconFolder = ({ size }: P) => (<svg {...s(size)} strokeWidth={1.8} className="fi"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z" /></svg>);
export const IconRefresh = ({ size }: P) => (<svg {...s(size)}><path d="M21 12a9 9 0 1 1-2.6-6.4M21 4v5h-5" /></svg>);
export const IconMoon = ({ size }: P) => (<svg {...s(size)}><path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z" /></svg>);
export const IconMsg = ({ size }: P) => (<svg {...s(size)}><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" /></svg>);
export const IconClock = ({ size }: P) => (<svg {...s(size)}><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></svg>);
export const IconCopy = ({ size }: P) => (<svg {...s(size)}><rect x="9" y="9" width="11" height="11" rx="2" /><path d="M5 15V5a2 2 0 0 1 2-2h10" /></svg>);
export const IconTerminal = ({ size }: P) => (<svg {...s(size)}><rect x="3" y="4" width="18" height="16" rx="2" /><path d="m7 9 3 3-3 3M13 15h4" /></svg>);
export const IconChevron = ({ size }: P) => (<svg {...s(size)} strokeWidth={2.5}><path d="m9 6 6 6-6 6" /></svg>);
export const IconChevronDown = ({ size }: P) => (<svg {...s(size)} strokeWidth={2.4}><path d="m6 9 6 6 6-6" /></svg>);
export const IconTool = ({ size }: P) => (<svg {...s(size)} stroke="var(--text-lo)"><path d="m8 6-6 6 6 6M16 6l6 6-6 6" /></svg>);
export const IconLogo = ({ size }: P) => (<svg {...s(size)} strokeWidth={2.4}><path d="M11 4 4 11l7 7M4 11h10a6 6 0 0 1 6 6v0" /></svg>);
export const IconCheck = ({ size }: P) => (<svg {...s(size)} strokeWidth={3}><path d="M5 12l5 5 9-11" /></svg>);
export const IconPlay = ({ size }: P) => (<svg {...s(size)} strokeWidth={2.4}><path d="m6 4 13 8-13 8V4Z" /></svg>);
export const IconTrash = ({ size }: P) => (<svg {...s(size)}><path d="M4 7h16M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2m2 0v12a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V7" /></svg>);
export const IconEyeOff = ({ size }: P) => (<svg {...s(size)}><path d="M3 3l18 18M10.6 10.6a2 2 0 0 0 2.8 2.8M9.4 5.2A9 9 0 0 1 21 12a9.7 9.7 0 0 1-2.3 3M6.1 6.1A9.7 9.7 0 0 0 3 12a9 9 0 0 0 11 6.6" /></svg>);
export const IconEye = ({ size }: P) => (<svg {...s(size)}><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7Z" /><circle cx="12" cy="12" r="3" /></svg>);
export const IconReveal = ({ size }: P) => (<svg {...s(size)}><path d="M9 5H6a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2v-3M14 4h6v6M20 4l-9 9" /></svg>);
const STAR = "M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 17.9 6.8 20.6l1-5.8L3.5 9.7l5.9-.9L12 3.5Z";
export const IconStar = ({ size }: P) => (<svg {...s(size)} strokeWidth={1.8}><path d={STAR} /></svg>);
export const IconStarFilled = ({ size }: P) => (<svg {...s(size)} fill="currentColor" stroke="none"><path d={STAR} /></svg>);
export const IconSliders = ({ size }: P) => (<svg {...s(size)}><path d="M4 6h10M18 6h2M4 12h2M10 12h10M4 18h7M15 18h5" /><circle cx="15" cy="6" r="2" /><circle cx="8" cy="12" r="2" /><circle cx="13" cy="18" r="2" /></svg>);
