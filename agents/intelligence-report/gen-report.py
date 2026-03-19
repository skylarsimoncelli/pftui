#!/usr/bin/env python3
"""Generate branded PFTUI Intelligence Report PDFs."""
import sys
import markdown
from weasyprint import HTML

# pftui.com brand: dark theme, Inter/JetBrains Mono, green-cyan-blue gradient accents
CSS = """
@import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;600&display=swap');

@page {
    size: A4;
    margin: 2cm 2.2cm;
    background: #0d1117;

    @top-left {
        content: "PFTUI Intelligence Report";
        font-family: 'JetBrains Mono', monospace;
        font-size: 7.5pt;
        color: #6e7681;
        padding-top: 0.5cm;
    }
    @top-right {
        content: "CONFIDENTIAL";
        font-family: 'JetBrains Mono', monospace;
        font-size: 7.5pt;
        color: #f38ba8;
        padding-top: 0.5cm;
    }
    @bottom-center {
        content: counter(page) " / " counter(pages);
        font-family: 'JetBrains Mono', monospace;
        font-size: 7.5pt;
        color: #6e7681;
    }
}

@page :first {
    @top-left { content: none; }
    @top-right { content: none; }
}

body {
    font-family: 'Inter', -apple-system, BlinkMacSystemFont, sans-serif;
    font-size: 10pt;
    line-height: 1.65;
    color: #c9d1d9;
    background: #0d1117;
}

/* Cover header */
.report-header {
    text-align: center;
    padding: 1.5cm 0 1cm 0;
    margin-bottom: 0.8cm;
    border-bottom: 2px solid #30363d;
}

.report-brand {
    font-family: 'JetBrains Mono', monospace;
    font-size: 10pt;
    font-weight: 600;
    letter-spacing: 0.15em;
    color: #89dceb;
    text-transform: uppercase;
    margin-bottom: 0.3cm;
}

.report-title {
    font-size: 22pt;
    font-weight: 700;
    color: #c9d1d9;
    line-height: 1.2;
    margin-bottom: 0.3cm;
}

.report-date {
    font-family: 'JetBrains Mono', monospace;
    font-size: 9pt;
    color: #8b949e;
}

.report-classification {
    display: inline-block;
    font-family: 'JetBrains Mono', monospace;
    font-size: 7.5pt;
    color: #f38ba8;
    border: 1px solid rgba(243, 139, 168, 0.35);
    border-radius: 4px;
    padding: 2px 8px;
    margin-top: 0.3cm;
    letter-spacing: 0.08em;
}

/* Headings */
h1 {
    font-size: 18pt;
    font-weight: 700;
    color: #c9d1d9;
    border-bottom: 2px solid #89dceb;
    padding-bottom: 6px;
    margin-top: 0;
    margin-bottom: 12px;
    page-break-after: avoid;
}

h2 {
    font-size: 13pt;
    font-weight: 600;
    color: #89dceb;
    border-bottom: 1px solid #30363d;
    padding-bottom: 4px;
    margin-top: 20px;
    margin-bottom: 10px;
    page-break-after: avoid;
}

h3 {
    font-size: 11pt;
    font-weight: 600;
    color: #a6e3a1;
    margin-top: 16px;
    margin-bottom: 6px;
    page-break-after: avoid;
}

h4 {
    font-size: 10pt;
    font-weight: 600;
    color: #89b4fa;
    margin-top: 12px;
    margin-bottom: 4px;
}

/* Tables */
table {
    border-collapse: collapse;
    width: 100%;
    margin: 10px 0;
    font-size: 9pt;
    page-break-inside: avoid;
}

th, td {
    border: 1px solid #30363d;
    padding: 6px 10px;
    text-align: left;
}

th {
    background: #161b22;
    font-weight: 600;
    color: #89dceb;
    font-size: 8.5pt;
    text-transform: uppercase;
    letter-spacing: 0.03em;
}

td {
    background: #0d1117;
    color: #c9d1d9;
}

tr:nth-child(even) td {
    background: #161b22;
}

/* Blockquotes */
blockquote {
    border-left: 3px solid #89dceb;
    margin: 12px 0;
    padding: 8px 14px;
    background: rgba(137, 220, 235, 0.06);
    color: #c9d1d9;
    font-style: italic;
}

/* Code */
code {
    font-family: 'JetBrains Mono', monospace;
    background: #161b22;
    color: #a6e3a1;
    padding: 1px 5px;
    border-radius: 3px;
    font-size: 8.5pt;
    border: 1px solid #30363d;
}

pre {
    background: #161b22;
    border: 1px solid #30363d;
    border-radius: 6px;
    padding: 10px 14px;
    font-family: 'JetBrains Mono', monospace;
    font-size: 8.5pt;
    color: #c9d1d9;
    overflow-wrap: break-word;
    white-space: pre-wrap;
    page-break-inside: avoid;
}

pre code {
    background: none;
    border: none;
    padding: 0;
    color: inherit;
}

/* Strong / emphasis */
strong {
    color: #c9d1d9;
    font-weight: 600;
}

em {
    color: #8b949e;
}

/* Horizontal rules */
hr {
    border: none;
    border-top: 1px solid #30363d;
    margin: 18px 0;
}

/* Lists */
ul, ol {
    padding-left: 1.4em;
    margin: 6px 0;
}

li {
    margin-bottom: 4px;
}

/* Links */
a {
    color: #89b4fa;
    text-decoration: none;
}

/* Images — fit within page margins */
img {
    max-width: 100%;
    height: auto;
    display: block;
    margin: 12px auto;
    page-break-inside: avoid;
}

/* Star marker for most likely scenario */
p, li {
    orphans: 3;
    widows: 3;
}

/* Footer */
.report-footer {
    margin-top: 1cm;
    padding-top: 0.4cm;
    border-top: 1px solid #30363d;
    text-align: center;
    font-family: 'JetBrains Mono', monospace;
    font-size: 7.5pt;
    color: #6e7681;
}
"""

def md_to_pdf(md_path, pdf_path, title, date, subtitle=None, author="Skylar Simoncelli"):
    with open(md_path, 'r') as f:
        md_content = f.read()

    # Strip the first H1 if present (we use the custom header instead)
    lines = md_content.split('\n')
    if lines and lines[0].startswith('# '):
        lines = lines[1:]
    # Strip any "### date" line right after
    while lines and (lines[0].strip() == '' or lines[0].startswith('### ')):
        if lines[0].startswith('### '):
            lines = lines[1:]
        elif lines[0].strip() == '':
            lines = lines[1:]
        else:
            break
    md_content = '\n'.join(lines)

    # Resolve relative image paths to absolute file:// URIs for WeasyPrint
    import os
    md_dir = os.path.dirname(os.path.abspath(md_path))
    import re
    def resolve_img(match):
        alt = match.group(1)
        src = match.group(2)
        if not src.startswith(('http://', 'https://', 'file://', '/')):
            src = 'file://' + os.path.join(md_dir, src)
        return f'![{alt}]({src})'
    md_content = re.sub(r'!\[([^\]]*)\]\(([^)]+)\)', resolve_img, md_content)

    html_body = markdown.markdown(md_content, extensions=['tables', 'fenced_code'])

    sub_html = f'<div style="font-size: 10pt; color: #8b949e; margin-top: 0.2cm;">{subtitle}</div>' if subtitle else ''
    author_html = f'<div style="font-family: \'JetBrains Mono\', monospace; font-size: 9pt; color: #8b949e; margin-top: 0.15cm;">By {author}</div>' if author else ''

    html_doc = f"""<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><style>{CSS}</style></head>
<body>
<div class="report-header">
    <div class="report-brand">PFTUI Intelligence Report</div>
    <div class="report-title">{title}</div>
    <div class="report-date">{date}</div>
    {sub_html}
    {author_html}
    <div class="report-classification">Confidential</div>
</div>
{html_body}
<div class="report-footer">
    Generated by Sentinel Intelligence System | pftui.com
</div>
</body>
</html>"""

    HTML(string=html_doc).write_pdf(pdf_path)
    print(f"Generated: {pdf_path}")

if __name__ == '__main__':
    md_path = sys.argv[1]
    pdf_path = sys.argv[2]
    title = sys.argv[3]
    date = sys.argv[4]
    subtitle = sys.argv[5] if len(sys.argv) > 5 else None
    author = sys.argv[6] if len(sys.argv) > 6 else "Skylar Simoncelli"
    md_to_pdf(md_path, pdf_path, title, date, subtitle, author)
