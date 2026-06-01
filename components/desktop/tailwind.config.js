/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{ts,tsx,js,jsx}'],
  theme: {
    extend: {
      colors: {
        vv: {
          bg: '#0B0F19',
          bg2: '#080B10',
          rail: '#12161D',
          panel: '#151922',
          panel2: '#10151F',
          line: 'rgba(255,255,255,0.095)',
          line2: 'rgba(255,255,255,0.16)',
          text: '#ECEFF4',
          muted: '#878D98',
          dim: '#555B66',
          cyan: '#75D7FF',
          cyan2: '#2EAADC',
          green: '#5EE0B5',
          amber: '#F0B45B',
          red: '#FF4F6D',
          pink: '#FF2E7E',
        },
      },
      boxShadow: {
        glow: '0 0 0 1px rgba(117,215,255,0.35), 0 0 34px rgba(117,215,255,0.12)',
        danger: '0 0 0 1px rgba(255,79,109,0.35), 0 0 34px rgba(255,79,109,0.10)',
      },
      fontFamily: {
        sans: ['Inter', 'ui-sans-serif', 'system-ui', 'sans-serif'],
        mono: ['JetBrains Mono', 'SFMono-Regular', 'ui-monospace', 'monospace'],
      },
      backgroundImage: {
        'vv-radial': 'radial-gradient(circle at 50% 0%, rgba(117,215,255,0.09), transparent 34rem)',
        'vv-grid': 'linear-gradient(rgba(255,255,255,0.018) 1px, transparent 1px), linear-gradient(90deg, rgba(255,255,255,0.018) 1px, transparent 1px)',
      },
    },
  },
  plugins: [],
};
