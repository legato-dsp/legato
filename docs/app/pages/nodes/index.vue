<script setup lang="ts">
const { data } = await useAsyncData("nodes", () =>
  queryCollection("nodes").all()
);
const nodes = computed(() => data.value ?? []);
</script>

<template>
  <div class="flex flex-col gap-3 overflow-auto w-full">
    <div
      class="border border-primary rounded-md p-3 flex flex-col"
      v-for="item in nodes"
    >
      <h6>{{ item.name }}</h6>
      <span>{{ item.description }}</span>
      <span>Required params: {{ item.required_params.join(" ") }}</span>
      <span>Optional params: {{ item.optional_params.join(" ") }}</span>
    </div>
  </div>
</template>
