import { NextRequest, NextResponse } from 'next/server';
import fs from 'fs';
import path from 'path';

export const dynamic = 'force-dynamic';

// GET /api/recordings/download?file=name.csv - download a recording
export async function GET(request: NextRequest) {
  try {
    const searchParams = request.nextUrl.searchParams;
    const file = searchParams.get('file');
    if (!file) {
      return NextResponse.json({ error: 'Missing file parameter' }, { status: 400 });
    }

    // Prevent path traversal
    if (file.includes('..') || file.includes('/') || file.includes('\\')) {
      return NextResponse.json({ error: 'Invalid file name' }, { status: 400 });
    }

    const recordingsDir = path.join(process.cwd(), '..', 'recordings');
    const filePath = path.join(recordingsDir, file);
    if (!fs.existsSync(filePath)) {
      return NextResponse.json({ error: 'File not found' }, { status: 404 });
    }

    const data = fs.readFileSync(filePath);
    const res = new NextResponse(data);
    res.headers.set('Content-Type', 'text/csv');
    res.headers.set('Content-Disposition', `attachment; filename="${file}"`);
    return res;
  } catch (error) {
    console.error('Error downloading recording:', error);
    return NextResponse.json({ error: 'Failed to download recording' }, { status: 500 });
  }
}

