import type { Metadata } from "next";
import { Open_Sans } from "next/font/google";
import "./globals.css";
import { AppProviders } from "./providers";

// Load Open Sans - perfect for medical interfaces and small screens
const openSans = Open_Sans({
  variable: "--font-open-sans",
  subsets: ["latin"],
  weight: ["400", "500", "600", "700"],
});

export const metadata: Metadata = {
  title: "Elata EEG Monitor",
  description: "Real-time EEG monitoring application",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body
        className={`${openSans.variable} antialiased`}
        style={{
          fontFamily: 'var(--font-family-system)',
          backgroundColor: 'var(--color-background)',
          color: 'var(--color-foreground)'
        }}
      >
        <AppProviders>{children}</AppProviders>
      </body>
    </html>
  );
}
