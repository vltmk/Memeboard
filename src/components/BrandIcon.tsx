import React from 'react';
import { cn } from '../lib/utils';

interface BrandIconProps {
  className?: string;
}

export const BrandIcon: React.FC<BrandIconProps> = ({ className }) => (
  <svg
    viewBox="0 0 24 24"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    className={cn('w-4 h-4', className)}
    aria-hidden="true"
  >
    <rect x="3" y="9" width="2.4" height="6" rx="1.2" fill="currentColor" opacity="0.45" />
    <rect x="6.9" y="6" width="2.4" height="12" rx="1.2" fill="currentColor" opacity="0.78" />
    <rect x="10.8" y="3.5" width="2.4" height="17" rx="1.2" fill="currentColor" />
    <rect x="14.7" y="6" width="2.4" height="12" rx="1.2" fill="currentColor" opacity="0.78" />
    <rect x="18.6" y="9" width="2.4" height="6" rx="1.2" fill="currentColor" opacity="0.45" />
  </svg>
);
