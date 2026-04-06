import { computed, ref } from 'vue';
import { fetchCurrentUser, type CurrentUserResponse } from '../api/users';
import { hasReadOnlyAttribute } from '../lib/readOnlyUi';

const currentUser = ref<CurrentUserResponse | null>(null);
const currentUserLoading = ref(false);
const currentUserLoaded = ref(false);
const currentUserError = ref('');

/**
 * 現在ユーザ情報を共有状態として扱う composable。
 *
 * @returns 現在ユーザ情報と、その読込制御関数
 */
export function useCurrentUser() {
  const isReadOnlyUser = computed(() =>
    hasReadOnlyAttribute(currentUser.value?.attributes),
  );

  async function loadCurrentUser(force = false): Promise<void> {
    if (currentUserLoading.value) {
      return;
    }
    if (currentUserLoaded.value && !force) {
      return;
    }

    currentUserLoading.value = true;
    currentUserError.value = '';
    try {
      currentUser.value = await fetchCurrentUser();
      currentUserLoaded.value = true;
    } catch (err) {
      currentUser.value = null;
      currentUserLoaded.value = false;
      currentUserError.value = err instanceof Error ? err.message : String(err);
    } finally {
      currentUserLoading.value = false;
    }
  }

  return {
    currentUser,
    currentUserLoading,
    currentUserLoaded,
    currentUserError,
    isReadOnlyUser,
    loadCurrentUser,
  };
}
