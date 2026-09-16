export function useAuth() {
  return {
    isSignedIn: false,
    isGracePeriodOnly: false,
    isLoaded: true,
    session: null,
    user: null,
    refetch: async () => null,
  };
}
