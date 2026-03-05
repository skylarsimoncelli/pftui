// ===== COPY TO CLIPBOARD =====
function copyInstall() {
    const text = document.getElementById('install-cmd').textContent;
    copyToClipboard(text);
}

function copyCode(button) {
    const codeBlock = button.previousElementSibling;
    const text = codeBlock.textContent;
    copyToClipboard(text);
    
    const originalText = button.textContent;
    button.textContent = 'Copied!';
    button.style.background = 'var(--accent-green)';
    button.style.color = 'var(--bg-primary)';
    
    setTimeout(() => {
        button.textContent = originalText;
        button.style.background = '';
        button.style.color = '';
    }, 2000);
}

function copyToClipboard(text) {
    if (navigator.clipboard && window.isSecureContext) {
        navigator.clipboard.writeText(text);
    } else {
        const textArea = document.createElement('textarea');
        textArea.value = text;
        textArea.style.position = 'fixed';
        textArea.style.left = '-999999px';
        document.body.appendChild(textArea);
        textArea.select();
        try { document.execCommand('copy'); } catch (e) {}
        document.body.removeChild(textArea);
    }
}

// ===== TERMINAL DEMO =====
// Multi-scene terminal that cycles through pftui features

const scenes = [
    {
        lines: [
            { text: '$ pftui', type: 'command', delay: 60 },
            { text: '', type: 'output', delay: 300 },
            { text: '┌─ Portfolio ──────────────────────────────┐', type: 'output', delay: 30 },
            { text: '│  Total Value    $48,217    ▲ +2.4%       │', type: 'output', delay: 30 },
            { text: '│  Day P&L        +$1,132                  │', type: 'output', delay: 30 },
            { text: '│                                          │', type: 'output', delay: 30 },
            { text: '│  Equity   45%  ██████████░░░░░░░░░░░░   │', type: 'output', delay: 30 },
            { text: '│  Crypto   30%  ██████░░░░░░░░░░░░░░░░   │', type: 'output', delay: 30 },
            { text: '│  Comd     25%  █████░░░░░░░░░░░░░░░░░   │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui macro', type: 'command', delay: 60 },
            { text: '', type: 'output', delay: 200 },
            { text: '┌─ Market Intelligence ────────────────────┐', type: 'output', delay: 30 },
            { text: '│  S&P 500   5,842   ▲ +0.8%    RSI 58    │', type: 'output', delay: 30 },
            { text: '│  VIX         14.2   ▼ -10.3%             │', type: 'output', delay: 30 },
            { text: '│  DXY         98.8   ▼ -0.1%    RSI 42    │', type: 'output', delay: 30 },
            { text: '│  Gold      2,847   ▲ +1.2%    RSI 67    │', type: 'output', delay: 30 },
            { text: '│  10Y Yield  4.21%  ▼ -3bps               │', type: 'output', delay: 30 },
            { text: '│  Oil WTI    71.40   ▲ +0.6%              │', type: 'output', delay: 30 },
            { text: '│  BTC       97,420   ▲ +3.1%    RSI 61    │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui brief --agent', type: 'command', delay: 60 },
            { text: '', type: 'output', delay: 200 },
            { text: '{', type: 'json', delay: 20 },
            { text: '  "portfolio_value": 48217.43,', type: 'json', delay: 20 },
            { text: '  "daily_pnl": 1132.18,', type: 'json', delay: 20 },
            { text: '  "daily_pnl_pct": 2.4,', type: 'json', delay: 20 },
            { text: '  "top_movers": [', type: 'json', delay: 20 },
            { text: '    {"symbol": "BTC", "change": 3.1},', type: 'json', delay: 20 },
            { text: '    {"symbol": "GOLD", "change": 1.2}', type: 'json', delay: 20 },
            { text: '  ],', type: 'json', delay: 20 },
            { text: '  "alerts_triggered": 0', type: 'json', delay: 20 },
            { text: '}', type: 'json', delay: 20 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui predictions', type: 'command', delay: 60 },
            { text: '', type: 'output', delay: 200 },
            { text: '┌─ Prediction Markets ─────────────────────┐', type: 'output', delay: 30 },
            { text: '│  Fed rate cut by June?        67%   ▲+4  │', type: 'output', delay: 30 },
            { text: '│  BTC above $100k by Q2?       52%   ▼-3  │', type: 'output', delay: 30 },
            { text: '│  US recession in 2026?        23%   ▲+2  │', type: 'output', delay: 30 },
            { text: '│  Gold above $3,000?           78%   ▲+6  │', type: 'output', delay: 30 },
            { text: '│  Trump tariffs expanded?      61%   →0   │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
];

let currentScene = 0;
let terminal = null;
let cursor = null;
let isAnimating = false;

function clearTerminal() {
    const lines = terminal.querySelectorAll('.terminal-line');
    lines.forEach(l => l.remove());
}

function createLine(text, type) {
    const line = document.createElement('div');
    line.className = 'terminal-line';
    
    if (type === 'command') {
        line.style.color = 'var(--accent-cyan)';
        line.style.fontWeight = '600';
    } else if (type === 'json') {
        line.style.color = 'var(--accent-yellow)';
        line.style.fontSize = '0.85em';
    }
    
    // Color code specific characters
    if (type === 'output' || type === 'json') {
        line.innerHTML = colorize(text);
    }
    
    return line;
}

function colorize(text) {
    return text
        .replace(/▲ \+[\d.]+%?/g, '<span style="color:var(--accent-green)">$&</span>')
        .replace(/▲\+\d+/g, '<span style="color:var(--accent-green)">$&</span>')
        .replace(/▼ -[\d.]+%?/g, '<span style="color:var(--accent-red)">$&</span>')
        .replace(/▼-\d+/g, '<span style="color:var(--accent-red)">$&</span>')
        .replace(/→0/g, '<span style="color:var(--text-tertiary)">$&</span>')
        .replace(/(RSI \d+)/g, '<span style="color:var(--accent-blue)">$1</span>')
        .replace(/(█+)/g, '<span style="color:var(--accent-green)">$1</span>')
        .replace(/(░+)/g, '<span style="color:var(--bg-tertiary)">$1</span>')
        .replace(/(\d+%)\s/g, '<span style="color:var(--accent-cyan)">$1</span> ')
        .replace(/([\d,]+\.\d+|\$[\d,]+)/g, '<span style="color:var(--text-primary);font-weight:500">$1</span>');
}

async function typeText(element, text, delay) {
    for (let i = 0; i <= text.length; i++) {
        element.textContent = text.slice(0, i);
        await sleep(delay);
    }
}

async function playScene(scene) {
    clearTerminal();
    cursor.style.display = 'inline-block';
    
    for (const line of scene.lines) {
        const el = createLine('', line.type);
        terminal.insertBefore(el, cursor);
        
        if (line.type === 'command') {
            // Type out commands character by character
            await typeText(el, line.text, line.delay);
            await sleep(400);
            cursor.style.display = 'none';
            await sleep(200);
        } else {
            // Output lines appear instantly (like real terminal output)
            if (line.type === 'json') {
                el.innerHTML = line.text;
                el.style.color = 'var(--accent-yellow)';
            } else {
                el.innerHTML = colorize(line.text);
            }
            await sleep(line.delay);
        }
    }
    
    // Show cursor blinking after scene completes
    cursor.style.display = 'inline-block';
    
    // Hold the scene
    await sleep(scene.hold);
}

async function runTerminalLoop() {
    if (isAnimating) return;
    isAnimating = true;
    
    while (true) {
        await playScene(scenes[currentScene]);
        
        // Fade transition
        terminal.style.opacity = '0.3';
        await sleep(300);
        terminal.style.opacity = '1';
        
        currentScene = (currentScene + 1) % scenes.length;
    }
}

function sleep(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
}

// ===== INTERSECTION OBSERVER FOR ANIMATIONS =====
const observerOptions = {
    threshold: 0.1,
    rootMargin: '0px 0px -50px 0px'
};

const observer = new IntersectionObserver((entries) => {
    entries.forEach(entry => {
        if (entry.isIntersecting) {
            entry.target.classList.add('visible');
        }
    });
}, observerOptions);

// ===== SMOOTH SCROLL =====
document.querySelectorAll('a[href^="#"]').forEach(anchor => {
    anchor.addEventListener('click', function (e) {
        e.preventDefault();
        const target = document.querySelector(this.getAttribute('href'));
        if (target) {
            target.scrollIntoView({ behavior: 'smooth', block: 'start' });
        }
    });
});

// ===== INIT =====
window.addEventListener('DOMContentLoaded', () => {
    terminal = document.getElementById('terminal');
    cursor = terminal.querySelector('.terminal-cursor');
    
    // Add transition for fade effect
    terminal.style.transition = 'opacity 0.3s ease';
    
    // Start terminal demo after a brief pause
    setTimeout(runTerminalLoop, 800);
    
    // Observe fade-in elements
    document.querySelectorAll('.fade-in, .feature-card').forEach(el => {
        observer.observe(el);
    });
});
