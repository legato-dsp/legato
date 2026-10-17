<script setup lang="ts">
const { data: posts } = await useAsyncData("blog", () =>
  queryCollection("blog").order("date", "DESC").all()
);
</script>

<template>
  <div class="w-full h-full flex justify-center">
    <div class="w-full max-w-200 h-full overflow-auto flex flex-col gap-8">
      <NuxtLink
        v-for="post in posts"
        :key="post.id"
        :to="post.path"
        class="flex flex-col gap-1 no-underline"
      >
        <p class="text-sm">{{ post.date }}</p>
        <h2 class="text-2xl">{{ post.title }}</h2>
        <p>{{ post.description }}</p>
      </NuxtLink>
    </div>
  </div>
</template>
