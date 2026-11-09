import { cva, type VariantProps } from 'class-variance-authority'
export { default as Button } from './Button.vue'

export const buttonVariants = cva(
  'inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-semibold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:pointer-events-none disabled:opacity-45',
  {
    variants: {
      variant: {
        default: 'bg-primary text-primary-foreground hover:bg-primary/85',
        secondary: 'border border-border bg-secondary text-secondary-foreground hover:bg-accent',
        ghost: 'text-muted-foreground hover:bg-accent hover:text-foreground',
        destructive: 'border border-destructive/40 text-destructive hover:bg-destructive/10',
      },
      size: { default: 'h-10 px-4 py-2', sm: 'h-8 px-3 text-xs', icon: 'size-10' },
    },
    defaultVariants: { variant: 'default', size: 'default' },
  },
)
export type ButtonVariants = VariantProps<typeof buttonVariants>
