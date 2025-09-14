import EegMonitorWrapper from '@/components/EegMonitorWrapper';

export const dynamic = 'force-dynamic';

export default function Home() {
  return (
    <main className="flex flex-col h-screen" style={{
      backgroundColor: 'var(--color-off-cream)',
      color: 'var(--color-off-black)'
    }}>
      <EegMonitorWrapper />
    </main>
  );
}
