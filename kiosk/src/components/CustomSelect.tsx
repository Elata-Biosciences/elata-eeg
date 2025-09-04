'use client';

import React, { useState, useRef, useEffect } from 'react';

interface Option {
  value: string;
  label: string;
}

interface CustomSelectProps {
  id?: string;
  value: string;
  onChange: (value: string) => void;
  options: Option[];
  disabled?: boolean;
  placeholder?: string;
  onFocus?: () => void;
}

export default function CustomSelect({
  id,
  value,
  onChange,
  options,
  disabled = false,
  placeholder = 'Select...',
  onFocus
}: CustomSelectProps) {
  const [isOpen, setIsOpen] = useState(false);
  const selectRef = useRef<HTMLDivElement>(null);

  // Close dropdown when clicking outside
  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (selectRef.current && !selectRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    }

    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  const selectedOption = options.find(opt => opt.value === value);

  return (
    <div className="relative" ref={selectRef}>
      {/* Select Button */}
      <button
        type="button"
        onClick={() => {
          if (!disabled) {
            setIsOpen(!isOpen);
            if (onFocus) onFocus();
          }
        }}
        disabled={disabled}
        className="w-full flex items-center justify-between px-4 py-3 rounded-lg transition-all duration-200 focus:outline-none focus:ring-2"
        style={{
          backgroundColor: 'var(--color-white)',
          border: '1px solid var(--color-gray2)',
          color: 'var(--color-off-black)',
          fontFamily: 'var(--font-family-system)',
          fontSize: 'var(--font-size-base)',
          minHeight: '48px',
          cursor: disabled ? 'not-allowed' : 'pointer',
          opacity: disabled ? 0.5 : 1,
          boxShadow: isOpen ? '0 0 0 2px color-mix(in srgb, var(--color-elata-green) 20%, transparent)' : 'none'
        }}
      >
        <span style={{
          color: selectedOption ? 'var(--color-off-black)' : 'var(--color-gray3)'
        }}>
          {selectedOption ? selectedOption.label : placeholder}
        </span>
        
        <svg 
          className={`h-5 w-5 transition-transform duration-200 ${isOpen ? 'rotate-180' : ''}`}
          xmlns="http://www.w3.org/2000/svg" 
          fill="none" 
          viewBox="0 0 24 24" 
          stroke="currentColor"
          style={{ color: 'var(--color-gray3)' }}
        >
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
        </svg>
      </button>

      {/* Dropdown - opens upward */}
      {isOpen && !disabled && (
        <div 
          className="absolute z-50 w-full bottom-full mb-1 rounded-lg shadow-lg overflow-hidden"
          style={{
            backgroundColor: 'var(--color-white)',
            border: '1px solid var(--color-gray2)',
            boxShadow: '0 -10px 25px color-mix(in srgb, var(--color-gray3) 15%, transparent)'
          }}
        >
          <div className="max-h-60 overflow-y-auto">
            {options.map((option) => (
              <button
                key={option.value}
                type="button"
                onClick={() => {
                  onChange(option.value);
                  setIsOpen(false);
                }}
                className="w-full px-4 py-3 text-left transition-colors duration-150 focus:outline-none"
                style={{
                  backgroundColor: option.value === value 
                    ? 'color-mix(in srgb, var(--color-elata-green) 10%, var(--color-white))'
                    : 'var(--color-white)',
                  color: option.value === value 
                    ? 'var(--color-elata-green)'
                    : 'var(--color-off-black)',
                  fontFamily: 'var(--font-family-system)',
                  fontSize: 'var(--font-size-base)',
                  borderBottom: '1px solid color-mix(in srgb, var(--color-gray2) 20%, transparent)'
                }}
                onMouseEnter={(e) => {
                  if (option.value !== value) {
                    e.currentTarget.style.backgroundColor = 'color-mix(in srgb, var(--color-gray1) 30%, var(--color-white))';
                  }
                }}
                onMouseLeave={(e) => {
                  if (option.value !== value) {
                    e.currentTarget.style.backgroundColor = 'var(--color-white)';
                  }
                }}
              >
                {option.label}
                {option.value === value && (
                  <svg className="inline-block ml-2 h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 13l4 4L19 7" />
                  </svg>
                )}
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
